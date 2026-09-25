#![allow(dead_code)]
//! Shared in-memory MOBI fixture builder.
//!
//! Included by the crate's own unit tests (`#[cfg(test)] mod test_support;` in
//! `lib.rs`) and by `tests/extraction.rs`, so there is exactly one builder.
//! It emits real PalmDB + PalmDOC/MOBI + EXTH bytes; every knob maps onto one
//! of those layers.

use std::io::Write;

const NO_IMAGE: u32 = 0xFFFF_FFFF;

/// Builds a minimal but spec-shaped MOBI-family file.
///
/// Record 0 is a 16-byte PalmDOC header plus a MOBI header (configurable
/// length, default 232) with an EXTH block and the `full_name` title bytes
/// appended inside record 0; every [`MobiBuilder::push_record`] call appends
/// one trailing record (text, image, or arbitrary bytes).
pub struct MobiBuilder {
    name: Vec<u8>,
    pdb_type: [u8; 4],
    pdb_creator: [u8; 4],
    compression: u16,
    palmdoc_record_count: u16,
    encryption: u16,
    codepage: u32,
    file_version: u32,
    header_length: u32,
    full_name: Vec<u8>,
    locale: u32,
    first_image_index: u32,
    exth_flags: u32,
    exth_items: Vec<(u32, Vec<u8>)>,
    exth_len_padding: usize,
    corrupt_item: Option<usize>,
    extra_records: Vec<Vec<u8>>,
    truncate_to: Option<usize>,
}

impl Default for MobiBuilder {
    fn default() -> Self {
        Self {
            name: b"TestBook".to_vec(),
            pdb_type: *b"BOOK",
            pdb_creator: *b"MOBI",
            compression: 2,
            palmdoc_record_count: 1,
            encryption: 0,
            codepage: 65001,
            file_version: 8,
            header_length: 232,
            full_name: Vec::new(),
            locale: 0,
            first_image_index: NO_IMAGE,
            exth_flags: 0x40,
            exth_items: Vec::new(),
            exth_len_padding: 0,
            corrupt_item: None,
            extra_records: Vec::new(),
            truncate_to: None,
        }
    }
}

impl MobiBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Append an EXTH record (string values as raw bytes; repeatable ids may
    /// be added multiple times, in order).
    pub fn exth(mut self, id: u32, value: impl Into<Vec<u8>>) -> Self {
        self.exth_items.push((id, value.into()));
        self
    }

    /// `full_name` title bytes stored inside record 0.
    pub fn full_name(mut self, name: &str) -> Self {
        self.full_name = name.as_bytes().to_vec();
        self
    }

    /// MOBI `text_encoding` (65001 = UTF-8, anything else = Windows-1252).
    pub fn codepage(mut self, codepage: u32) -> Self {
        self.codepage = codepage;
        self
    }

    /// PalmDOC encryption type (informational; 2 = DRM, still parseable).
    pub fn encryption(mut self, encryption: u16) -> Self {
        self.encryption = encryption;
        self
    }

    /// PalmDOC compression type (informational only, never decompressed).
    pub fn compression(mut self, compression: u16) -> Self {
        self.compression = compression;
        self
    }

    /// MOBI `file_version` (6 = MOBI6, 8 = KF8/AZW3).
    pub fn file_version(mut self, version: u32) -> Self {
        self.file_version = version;
        self
    }

    /// MOBI header length; the EXTH block starts at `16 + header_length`.
    pub fn header_length(mut self, len: u32) -> Self {
        self.header_length = len;
        self
    }

    /// PalmDOC `record_count` (ignored for slicing; the PDB `nrec` owns it).
    pub fn palmdoc_record_count(mut self, count: u16) -> Self {
        self.palmdoc_record_count = count;
        self
    }

    /// MOBI `first_image_index` (`0xFFFF_FFFF` = no images).
    pub fn first_image_index(mut self, index: u32) -> Self {
        self.first_image_index = index;
        self
    }

    /// MOBI `exth_flags` (EXTH present iff `& 0x40`).
    pub fn exth_flags(mut self, flags: u32) -> Self {
        self.exth_flags = flags;
        self
    }

    /// MOBI locale identifier (low byte 9 = English fallback).
    pub fn locale(mut self, locale: u32) -> Self {
        self.locale = locale;
        self
    }

    /// PDB type/creator pair (default `BOOK`/`MOBI`).
    pub fn pdb_kind(mut self, db_type: [u8; 4], creator: [u8; 4]) -> Self {
        self.pdb_type = db_type;
        self.pdb_creator = creator;
        self
    }

    /// Extra bytes added to the EXTH block's declared length (padding
    /// tolerance).
    pub fn exth_len_padding(mut self, padding: usize) -> Self {
        self.exth_len_padding = padding;
        self
    }

    /// Append one trailing record (text, image, or arbitrary bytes).
    pub fn push_record(mut self, record: impl Into<Vec<u8>>) -> Self {
        self.extra_records.push(record.into());
        self
    }

    /// Emit EXTH item `index` with a corrupt (`< 8`) size while keeping its
    /// 8-byte footprint, so the following items stay aligned and parseable.
    pub fn corrupt_exth_item(mut self, index: usize) -> Self {
        self.corrupt_item = Some(index);
        self
    }

    /// Truncate the finished file to `n` bytes (malformed-input cases).
    pub fn truncate(mut self, n: usize) -> Self {
        self.truncate_to = Some(n);
        self
    }

    /// Serialize the file to bytes.
    pub fn bytes(&self) -> Vec<u8> {
        let record0 = self.record0();
        let mut records: Vec<&[u8]> = vec![&record0];
        for extra in &self.extra_records {
            records.push(extra);
        }
        let nrec = records.len();

        let mut out = vec![0u8; 78];
        let name_len = self.name.len().min(32);
        out[..name_len].copy_from_slice(&self.name[..name_len]);
        out[60..64].copy_from_slice(&self.pdb_type);
        out[64..68].copy_from_slice(&self.pdb_creator);
        out[76..78].copy_from_slice(&(nrec as u16).to_be_bytes());

        let mut offset = 78 + 8 * nrec;
        for record in &records {
            out.extend_from_slice(&(offset as u32).to_be_bytes());
            out.extend_from_slice(&[0u8; 4]);
            offset += record.len();
        }
        for record in records {
            out.extend_from_slice(record);
        }

        if let Some(n) = self.truncate_to {
            out.truncate(n);
        }
        out
    }

    /// Serialize to a temporary `.mobi` file.
    pub fn tempfile(&self) -> tempfile::NamedTempFile {
        tempfile_with(&self.bytes())
    }

    fn record0(&self) -> Vec<u8> {
        let exth_start = 16 + self.header_length as usize;
        let base_len = 132usize.max(exth_start);
        let mut record = vec![0u8; base_len];

        // PalmDOC header.
        record[0..2].copy_from_slice(&self.compression.to_be_bytes());
        record[8..10].copy_from_slice(&self.palmdoc_record_count.to_be_bytes());
        record[12..14].copy_from_slice(&self.encryption.to_be_bytes());

        // MOBI header (absolute record-0 offsets).
        record[16..20].copy_from_slice(b"MOBI");
        record[20..24].copy_from_slice(&self.header_length.to_be_bytes());
        record[28..32].copy_from_slice(&self.codepage.to_be_bytes());
        record[36..40].copy_from_slice(&self.file_version.to_be_bytes());
        record[92..96].copy_from_slice(&self.locale.to_be_bytes());
        record[108..112].copy_from_slice(&self.first_image_index.to_be_bytes());
        record[128..132].copy_from_slice(&self.exth_flags.to_be_bytes());

        // EXTH block.
        let mut exth = Vec::new();
        exth.extend_from_slice(b"EXTH");
        exth.extend_from_slice(&[0u8; 4]); // declared length, backpatched
        exth.extend_from_slice(&(self.exth_items.len() as u32).to_be_bytes());
        for (index, (id, value)) in self.exth_items.iter().enumerate() {
            exth.extend_from_slice(&id.to_be_bytes());
            if self.corrupt_item == Some(index) {
                exth.extend_from_slice(&4u32.to_be_bytes());
            } else {
                exth.extend_from_slice(&((value.len() + 8) as u32).to_be_bytes());
                exth.extend_from_slice(value);
            }
        }
        let declared = (exth.len() + self.exth_len_padding) as u32;
        exth[4..8].copy_from_slice(&declared.to_be_bytes());
        // Append EXTH at exactly `exth_start` (extending the record as needed).
        if record.len() < exth_start {
            record.resize(exth_start, 0);
        }
        record.truncate(exth_start);
        record.extend_from_slice(&exth);

        // `full_name` title bytes, with offset/length backpatched.
        let full_name_offset = record.len() as u32;
        record.extend_from_slice(&self.full_name);
        record[84..88].copy_from_slice(&full_name_offset.to_be_bytes());
        record[88..92].copy_from_slice(&(self.full_name.len() as u32).to_be_bytes());

        record
    }
}

/// Write arbitrary bytes to a temporary `.mobi` file (for hand-crafted
/// malformed / foreign-container inputs that the builder cannot express).
pub fn tempfile_with(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::Builder::new().suffix(".mobi").tempfile().unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
    file
}
