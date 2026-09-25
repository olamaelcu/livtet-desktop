//! Bounded PalmDB container parsing over `&[u8]`.
//!
//! The MOBI family rides inside a PalmDB wrapper: a 78-byte header followed
//! by `nrec` 8-byte record entries, then the records themselves. All slicing
//! uses the PDB `nrec` count — never the PalmDOC `record_count`, which
//! excludes image and index records — and every offset is bounds-checked so
//! malformed files are errors, never panics.

use crate::Result;
use crate::error::MobiError;

/// KFX (DRMION) container magic: a different format family entirely.
const KFX_MAGIC: &[u8; 8] = &[0xEA, 0x44, 0x52, 0x4D, 0x49, 0x4F, 0x4E, 0xEE];

/// Topaz container magic prefix.
const TOPAZ_MAGIC: &[u8; 3] = b"TPZ";

/// Minimum size of the fixed PalmDB header.
const PDB_HEADER_LEN: usize = 78;

/// Size of one record-list entry.
const RECORD_ENTRY_LEN: usize = 8;

/// Parsed PalmDB container: the raw records in order.
///
/// Borrows from the input buffer; record `i` spans
/// `raw[off_i..off_{i+1}]` with the last record running to EOF.
#[derive(Debug)]
pub struct Pdb<'a> {
    records: Vec<&'a [u8]>,
}

impl<'a> Pdb<'a> {
    /// Parse the PalmDB wrapper around `raw`.
    pub fn parse(raw: &'a [u8]) -> Result<Self> {
        if raw.len() >= KFX_MAGIC.len() && &raw[..KFX_MAGIC.len()] == KFX_MAGIC {
            return Err(MobiError::UnsupportedContainer("kfx"));
        }
        if raw.len() >= TOPAZ_MAGIC.len() && &raw[..TOPAZ_MAGIC.len()] == TOPAZ_MAGIC {
            return Err(MobiError::UnsupportedContainer("topaz"));
        }

        if raw.len() < PDB_HEADER_LEN {
            return Err(MobiError::Malformed(format!(
                "file too short for PalmDB header: {} bytes",
                raw.len()
            )));
        }

        let is_book_mobi = &raw[60..64] == b"BOOK" && &raw[64..68] == b"MOBI";
        let is_text_read = &raw[60..64] == b"TEXT" && &raw[64..68] == b"READ";
        if !is_book_mobi && !is_text_read {
            return Err(MobiError::NotMobi);
        }

        let nrec = u16::from_be_bytes([raw[76], raw[77]]) as usize;
        let table_end = PDB_HEADER_LEN
            .checked_add(
                nrec.checked_mul(RECORD_ENTRY_LEN)
                    .ok_or_else(|| MobiError::Malformed("record count overflows".to_string()))?,
            )
            .ok_or_else(|| MobiError::Malformed("record table overflows".to_string()))?;
        if raw.len() < table_end {
            return Err(MobiError::Malformed(format!(
                "file truncated inside PalmDB record table: {} < {table_end}",
                raw.len()
            )));
        }

        let mut offsets = Vec::with_capacity(nrec);
        for i in 0..nrec {
            let base = PDB_HEADER_LEN + i * RECORD_ENTRY_LEN;
            let off = u32::from_be_bytes([raw[base], raw[base + 1], raw[base + 2], raw[base + 3]])
                as usize;
            offsets.push(off);
        }

        // Record 0 must start at or after the end of the record table, every
        // offset must lie inside the file, and offsets must be monotonic
        // non-decreasing (zero-length records are legal).
        for (i, &off) in offsets.iter().enumerate() {
            if i == 0 && off < table_end {
                return Err(MobiError::Malformed(format!(
                    "first record offset {off} overlaps the PalmDB record table ending at {table_end}"
                )));
            }
            if off > raw.len() {
                return Err(MobiError::Malformed(format!(
                    "record {i} offset {off} exceeds file length {}",
                    raw.len()
                )));
            }
            if i > 0 && off < offsets[i - 1] {
                return Err(MobiError::Malformed(format!(
                    "record offsets not monotonic at record {i}: {off} < {}",
                    offsets[i - 1]
                )));
            }
        }

        let mut records = Vec::with_capacity(nrec);
        for (i, &off) in offsets.iter().enumerate() {
            let end = if i + 1 < nrec {
                offsets[i + 1]
            } else {
                raw.len()
            };
            records.push(&raw[off..end]);
        }
        Ok(Self { records })
    }

    /// Record count from the PDB header.
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// All records in order; record 0 is the PalmDOC/MOBI header.
    pub fn records(&self) -> &[&'a [u8]] {
        &self.records
    }
}
