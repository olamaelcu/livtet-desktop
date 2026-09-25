#![allow(dead_code)]
//! Shared in-memory EPUB fixture builder.
//!
//! Included by the crate's own unit tests (`#[cfg(test)] mod test_support;` in
//! `lib.rs`) and by `tests/extraction.rs`, so there is exactly one builder.

use std::io::{Cursor, Write};

use ::zip::write::SimpleFileOptions;
use ::zip::{CompressionMethod, ZipWriter};

/// Default location of the package document inside the archive.
pub const DEFAULT_OPF_PATH: &str = "OEBPS/content.opf";

/// Builds a minimal but spec-shaped EPUB from an OPF body and extra files.
pub struct EpubBuilder {
    opf: String,
    opf_path: String,
    files: Vec<(String, Vec<u8>)>,
    encryption: Option<String>,
    opf_utf16: bool,
    container: Option<String>,
    strip_central_directory: bool,
}

impl EpubBuilder {
    pub fn new(opf: impl Into<String>) -> Self {
        Self {
            opf: opf.into(),
            opf_path: DEFAULT_OPF_PATH.to_string(),
            files: Vec::new(),
            encryption: None,
            opf_utf16: false,
            container: None,
            strip_central_directory: false,
        }
    }

    /// Add an arbitrary archive entry (paths are relative to the archive root).
    pub fn file(mut self, path: impl Into<String>, data: impl Into<Vec<u8>>) -> Self {
        self.files.push((path.into(), data.into()));
        self
    }

    /// Override `META-INF/encryption.xml`.
    pub fn encryption(mut self, xml: impl Into<String>) -> Self {
        self.encryption = Some(xml.into());
        self
    }

    /// Write the OPF as UTF-16 (little-endian, with BOM).
    pub fn opf_utf16(mut self) -> Self {
        self.opf_utf16 = true;
        self
    }

    /// Place the OPF at a non-default archive path.
    pub fn opf_path(mut self, path: impl Into<String>) -> Self {
        self.opf_path = path.into();
        self
    }

    /// Override `META-INF/container.xml`.
    pub fn container(mut self, xml: impl Into<String>) -> Self {
        self.container = Some(xml.into());
        self
    }

    /// Drop the central directory, exercising the raw local-header fallback.
    pub fn strip_central_directory(mut self) -> Self {
        self.strip_central_directory = true;
        self
    }

    /// Serialize the EPUB to bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated =
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

            zip.start_file("mimetype", stored).unwrap();
            zip.write_all(b"application/epub+zip").unwrap();

            let container = self
                .container
                .clone()
                .unwrap_or_else(|| default_container(&self.opf_path));
            zip.start_file("META-INF/container.xml", deflated).unwrap();
            zip.write_all(container.as_bytes()).unwrap();

            if let Some(encryption) = &self.encryption {
                zip.start_file("META-INF/encryption.xml", deflated).unwrap();
                zip.write_all(encryption.as_bytes()).unwrap();
            }

            for (path, data) in &self.files {
                zip.start_file(path.clone(), deflated).unwrap();
                zip.write_all(data).unwrap();
            }

            zip.start_file(self.opf_path.clone(), deflated).unwrap();
            if self.opf_utf16 {
                let mut encoded = vec![0xFF, 0xFE];
                for unit in self.opf.encode_utf16() {
                    encoded.extend_from_slice(&unit.to_le_bytes());
                }
                zip.write_all(&encoded).unwrap();
            } else {
                zip.write_all(self.opf.as_bytes()).unwrap();
            }
            zip.finish().unwrap();
        }

        let mut bytes = cursor.into_inner();
        if self.strip_central_directory {
            bytes = strip_central_directory(&bytes);
        }
        bytes
    }

    /// Serialize to a temporary `.epub` file.
    pub fn tempfile(&self) -> tempfile::NamedTempFile {
        let mut file = tempfile::Builder::new().suffix(".epub").tempfile().unwrap();
        file.write_all(&self.bytes()).unwrap();
        file.flush().unwrap();
        file
    }
}

/// Assemble an OPF package document.
pub fn package(version: &str, metadata: &str, manifest_extra: &str, guide: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="{version}" unique-identifier="bookid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
{metadata}
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
{manifest_extra}
  </manifest>
  <spine><itemref idref="chapter"/></spine>
{guide}
</package>"#
    )
}

/// Assemble an EPUB3 OPF package document.
pub fn package3(metadata: &str, manifest_extra: &str, guide: &str) -> String {
    package("3.0", metadata, manifest_extra, guide)
}

pub(crate) fn default_container(opf_path: &str) -> String {
    format!(
        r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="{opf_path}" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#
    )
}

pub(crate) fn strip_central_directory(bytes: &[u8]) -> Vec<u8> {
    let mut end = bytes.len();
    while end >= 22 {
        let start = end - 22;
        if &bytes[start..start + 4] == b"PK\x05\x06" {
            let cd_offset =
                u32::from_le_bytes(bytes[start + 16..start + 20].try_into().unwrap()) as usize;
            return bytes[..cd_offset].to_vec();
        }
        end -= 1;
    }
    bytes.to_vec()
}
