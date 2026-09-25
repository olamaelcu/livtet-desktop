//! ZIP reading with a fallback for archives whose central directory is damaged
//! or missing.
//!
//! The primary path delegates to the [`zip`] crate. If that fails — a common
//! failure mode for EPUBs written by misbehaving tools — we scan the raw bytes
//! for local file headers instead. The fallback supports only STORED and
//! DEFLATED entries and refuses encrypted entries, path-traversal names, and
//! unsupported compression rather than returning garbage.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::Arc;

use ::zip::ZipArchive;
use flate2::read::DeflateDecoder;

use crate::error::EpubError;

/// Maximum uncompressed size accepted for a single archive entry.
///
/// A real book's cover is ~300 KB, so 64 MiB is generous for any legitimate
/// entry while still bounding memory: DEFLATE can expand its input ~1000×, so
/// an untrusted entry must never be read without a ceiling.
pub(crate) const MAX_ENTRY_BYTES: usize = 64 * 1024 * 1024;

/// Initial pre-allocation for reading an entry. Deliberately independent of the
/// untrusted declared size; the buffer grows on demand up to [`MAX_ENTRY_BYTES`].
const INITIAL_READ_CAPACITY: usize = 1 << 20;

/// An open EPUB archive. Entries are keyed by normalized (forward-slash) name.
pub struct Archive {
    names: Vec<String>,
    inner: Inner,
}

enum Inner {
    Central(Box<ZipArchive<SharedReader>>),
    Raw(RawArchive),
}

struct RawArchive {
    data: Arc<Vec<u8>>,
    entries: Vec<RawEntry>,
}

/// A cheaply cloneable reader over a shared byte buffer, so the archive bytes
/// can back both the central-directory path and the raw fallback without a
/// full copy.
struct SharedReader {
    data: Arc<Vec<u8>>,
    position: u64,
}

impl SharedReader {
    fn new(data: Arc<Vec<u8>>) -> Self {
        Self { data, position: 0 }
    }
}

impl Read for SharedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let start = self.position as usize;
        if start >= self.data.len() {
            return Ok(0);
        }
        // `saturating_add` keeps a bogus seek position from overflowing `usize`.
        let end = start.saturating_add(buf.len()).min(self.data.len());
        let written = end - start;
        buf[..written].copy_from_slice(&self.data[start..end]);
        self.position += written as u64;
        Ok(written)
    }
}

impl Seek for SharedReader {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let position = match target {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.data.len() as i64 + offset,
            SeekFrom::Current(offset) => self.position as i64 + offset,
        };
        if position < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "attempted to seek before the start of the archive",
            ));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

struct RawEntry {
    name: String,
    method: u16,
    data_offset: usize,
    compressed_size: usize,
    encrypted: bool,
    has_descriptor: bool,
}

impl Archive {
    /// Open the archive, falling back to a raw scan when the central directory
    /// is unusable.
    pub fn open(bytes: Vec<u8>) -> Result<Self, EpubError> {
        let data = Arc::new(bytes);
        match ZipArchive::new(SharedReader::new(Arc::clone(&data))) {
            Ok(archive) => {
                let names = archive
                    .file_names()
                    .map(normalize)
                    .filter(|name| is_safe_name(name))
                    .collect();
                Ok(Self {
                    names,
                    inner: Inner::Central(Box::new(archive)),
                })
            }
            Err(_) => {
                let raw = RawArchive::parse(data)?;
                Ok(Self {
                    names: raw.entries.iter().map(|e| e.name.clone()).collect(),
                    inner: Inner::Raw(raw),
                })
            }
        }
    }

    /// Whether an entry with this normalized name exists.
    pub fn contains(&self, name: &str) -> bool {
        self.names.iter().any(|entry| entry == name)
    }

    /// Every entry name in the archive, normalized to forward slashes.
    pub fn entries(&self) -> Vec<String> {
        self.names.clone()
    }

    /// Read an entry's decompressed bytes, or `None` if it is missing, is a
    /// directory, is encrypted, uses an unsupported compression method, or
    /// exceeds [`MAX_ENTRY_BYTES`].
    pub fn read(&mut self, name: &str) -> Option<Vec<u8>> {
        if !is_safe_name(name) {
            return None;
        }
        match &mut self.inner {
            Inner::Central(archive) => {
                let original = archive
                    .file_names()
                    .find(|n| normalize(n) == name)
                    .map(str::to_string)?;
                let mut file = archive.by_name(&original).ok()?;
                if file.encrypted() || file.is_dir() {
                    return None;
                }
                // Never trust the declared size for allocation; cap both the
                // pre-allocation and the number of bytes we are willing to read.
                let declared = file.size().min(MAX_ENTRY_BYTES as u64) as usize;
                let mut out = Vec::with_capacity(declared.min(INITIAL_READ_CAPACITY));
                (&mut file)
                    .take(MAX_ENTRY_BYTES as u64 + 1)
                    .read_to_end(&mut out)
                    .ok()?;
                if out.len() > MAX_ENTRY_BYTES {
                    return None;
                }
                Some(out)
            }
            Inner::Raw(raw) => {
                let entry = raw.entries.iter().find(|e| e.name == name)?;
                raw.decode(entry)
            }
        }
    }
}

impl RawArchive {
    fn parse(data: Arc<Vec<u8>>) -> Result<Self, EpubError> {
        let mut entries = Vec::new();
        let mut index = 0;
        while index + 4 <= data.len() {
            if &data[index..index + 4] == b"PK\x03\x04"
                && let Some((entry, next)) = parse_local_header(&data, index)
            {
                if entry.name.ends_with('/') {
                    index = next;
                    continue;
                }
                entries.push(entry);
                index = next;
                continue;
            }
            index += 1;
        }
        if entries.is_empty() {
            return Err(EpubError::Zip(
                "no usable local file headers found".to_string(),
            ));
        }
        Ok(Self { data, entries })
    }

    fn decode(&self, entry: &RawEntry) -> Option<Vec<u8>> {
        if entry.encrypted {
            return None;
        }
        let data = self.data.get(entry.data_offset..)?;
        match entry.method {
            0 => {
                let end = if entry.compressed_size > 0 {
                    entry.data_offset.checked_add(entry.compressed_size)?
                } else if entry.has_descriptor {
                    // A STORED entry with the data-descriptor flag carries no
                    // size in its local header; locate the `PK\x07\x08` marker.
                    stored_data_end(&self.data, entry.data_offset)?
                } else {
                    return None;
                };
                if end.checked_sub(entry.data_offset)? > MAX_ENTRY_BYTES {
                    return None;
                }
                Some(self.data.get(entry.data_offset..end)?.to_vec())
            }
            8 => {
                let bounded = match entry.compressed_size {
                    0 => data,
                    size => data.get(..size)?,
                };
                let mut out = Vec::new();
                DeflateDecoder::new(bounded)
                    .take(MAX_ENTRY_BYTES as u64 + 1)
                    .read_to_end(&mut out)
                    .ok()?;
                if out.len() > MAX_ENTRY_BYTES {
                    return None;
                }
                Some(out)
            }
            _ => None,
        }
    }
}

/// Best-effort scan for a `PK\x07\x08` data descriptor from `start`, bounded by
/// [`MAX_ENTRY_BYTES`]; returns the descriptor's offset, i.e. the end of the
/// STORED payload.
fn stored_data_end(data: &[u8], start: usize) -> Option<usize> {
    let end = start.checked_add(MAX_ENTRY_BYTES)?.min(data.len());
    let window = data.get(start..end)?;
    window
        .windows(4)
        .position(|bytes| bytes == b"PK\x07\x08")
        .map(|offset| start + offset)
}

fn parse_local_header(data: &[u8], offset: usize) -> Option<(RawEntry, usize)> {
    if offset + 30 > data.len() {
        return None;
    }
    let flags = u16::from_le_bytes([data[offset + 6], data[offset + 7]]);
    let method = u16::from_le_bytes([data[offset + 8], data[offset + 9]]);
    let compressed_size =
        u32::from_le_bytes(data[offset + 18..offset + 22].try_into().ok()?) as usize;
    let name_len = u16::from_le_bytes([data[offset + 26], data[offset + 27]]) as usize;
    let extra_len = u16::from_le_bytes([data[offset + 28], data[offset + 29]]) as usize;

    let name_start = offset + 30;
    let name_end = name_start.checked_add(name_len)?;
    if name_end > data.len() {
        return None;
    }
    let name = normalize(std::str::from_utf8(&data[name_start..name_end]).ok()?);
    if name.is_empty() || !is_safe_name(&name) {
        return None;
    }

    let data_offset = name_end.checked_add(extra_len)?;
    if data_offset > data.len() {
        return None;
    }
    let encrypted = flags & 0x0001 != 0;
    let has_descriptor = flags & 0x0008 != 0;
    let compressed_size = if has_descriptor { 0 } else { compressed_size };

    // Only advance past the data when we trust its size; otherwise let the scan
    // resume right after the header (data bytes are skipped byte-by-byte).
    // `checked_add` guards against a hostile declared size overflowing `usize`.
    let next = data_offset
        .checked_add(compressed_size)
        .filter(|&end| end <= data.len())
        .unwrap_or(data_offset);
    Some((
        RawEntry {
            name,
            method,
            data_offset,
            compressed_size,
            encrypted,
            has_descriptor,
        },
        next,
    ))
}

/// Normalize path separators to forward slashes.
pub(crate) fn normalize(name: &str) -> String {
    name.replace('\\', "/")
}

/// Reject absolute paths, Windows drive prefixes, and `..` segments.
fn is_safe_name(name: &str) -> bool {
    if name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    if name.len() >= 2 && name.as_bytes()[1] == b':' {
        return false;
    }
    !name.split('/').any(|part| part == "..")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::strip_central_directory;
    use ::zip::write::SimpleFileOptions;
    use ::zip::{CompressionMethod, ZipWriter};
    use std::io::{Cursor, Write};

    fn sample_zip() -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer.start_file("mimetype", stored).unwrap();
            writer.write_all(b"application/epub+zip").unwrap();
            writer.start_file("OEBPS/content.opf", deflated).unwrap();
            writer.write_all(b"<package/>").unwrap();
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    /// One local file header (and payload) with no central directory, so the
    /// raw fallback scanner handles it.
    fn raw_local_entry(
        name: &str,
        method: u16,
        flags: u16,
        payload: &[u8],
        declared_size: u32,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"PK\x03\x04");
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // mod time
        out.extend_from_slice(&0u16.to_le_bytes()); // mod date
        out.extend_from_slice(&0u32.to_le_bytes()); // crc32
        out.extend_from_slice(&declared_size.to_le_bytes()); // compressed size
        out.extend_from_slice(&declared_size.to_le_bytes()); // uncompressed size
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra len
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn reads_entries_via_central_directory() {
        let mut archive = Archive::open(sample_zip()).unwrap();
        assert!(archive.contains("mimetype"));
        assert!(archive.contains("OEBPS/content.opf"));
        assert_eq!(
            archive.read("mimetype").unwrap(),
            b"application/epub+zip".to_vec()
        );
        assert_eq!(
            archive.read("OEBPS/content.opf").unwrap(),
            b"<package/>".to_vec()
        );
        assert!(archive.read("missing").is_none());
    }

    #[test]
    fn falls_back_when_central_directory_is_missing() {
        let bytes = strip_central_directory(&sample_zip());
        let mut archive = Archive::open(bytes).unwrap();
        assert_eq!(
            archive.read("mimetype").unwrap(),
            b"application/epub+zip".to_vec()
        );
        assert_eq!(
            archive.read("OEBPS/content.opf").unwrap(),
            b"<package/>".to_vec()
        );
    }

    #[test]
    fn reads_stored_entry_with_data_descriptor() {
        let payload = b"descriptor-backed cover bytes";
        let mut bytes = raw_local_entry("OEBPS/cover.jpg", 0, 0x0008, payload, 0);
        bytes.extend_from_slice(b"PK\x07\x08");
        bytes.extend_from_slice(&0u32.to_le_bytes()); // crc32
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        let mut archive = Archive::open(bytes).unwrap();
        assert_eq!(archive.read("OEBPS/cover.jpg").unwrap(), payload.to_vec());
    }

    #[test]
    fn rejects_entry_declaring_oversized_payload() {
        let declared = MAX_ENTRY_BYTES as u32 + 1;
        let bytes = raw_local_entry("OEBPS/huge.bin", 0, 0, b"tiny", declared);
        let mut archive = Archive::open(bytes).unwrap();
        assert!(archive.read("OEBPS/huge.bin").is_none());
    }

    #[test]
    fn caps_deflate_expansion() {
        use flate2::Compression;
        use flate2::write::DeflateEncoder;

        // ~66 MiB of zeros compresses to a few tens of KB (well over the cap).
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        let chunk = vec![0u8; 1 << 20];
        for _ in 0..66 {
            encoder.write_all(&chunk).unwrap();
        }
        let compressed = encoder.finish().unwrap();
        assert!(
            compressed.len() < MAX_ENTRY_BYTES / 100,
            "expected a highly compressible payload, got {} bytes",
            compressed.len()
        );

        let bytes = raw_local_entry("OEBPS/bomb.bin", 8, 0, &compressed, compressed.len() as u32);
        let mut archive = Archive::open(bytes).unwrap();
        assert!(archive.read("OEBPS/bomb.bin").is_none());
    }

    #[test]
    fn central_directory_caps_deflate_expansion() {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut writer = ZipWriter::new(&mut buffer);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            writer.start_file("OEBPS/bomb.bin", deflated).unwrap();
            let chunk = vec![0u8; 1 << 20];
            for _ in 0..66 {
                writer.write_all(&chunk).unwrap();
            }
            writer.finish().unwrap();
        }
        let mut archive = Archive::open(buffer.into_inner()).unwrap();
        assert!(archive.read("OEBPS/bomb.bin").is_none());
    }

    #[test]
    fn rejects_path_traversal_names() {
        assert!(!is_safe_name("../etc/passwd"));
        assert!(!is_safe_name("/etc/passwd"));
        assert!(!is_safe_name("C:/Windows/system32"));
        assert!(is_safe_name("OEBPS/content.opf"));
    }

    #[test]
    fn normalizes_backslashes() {
        assert_eq!(normalize("OEBPS\\content.opf"), "OEBPS/content.opf");
    }

    #[test]
    fn lists_entry_names() {
        let archive = Archive::open(sample_zip()).unwrap();
        let mut entries = archive.entries();
        entries.sort();
        assert_eq!(entries, vec!["OEBPS/content.opf", "mimetype"]);
    }
}
