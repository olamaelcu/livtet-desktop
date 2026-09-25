//! Record 0 parsing: PalmDOC header, MOBI header, and the EXTH block.
//!
//! All offsets are absolute within record 0. Compression is informational
//! only (never decompress) and a non-zero encryption type is recorded, not
//! rejected: DRM encrypts text records while EXTH metadata and covers stay
//! plaintext.

use crate::Result;
use crate::error::MobiError;

/// Minimum record 0 length covering every fixed field read below.
const MIN_RECORD0_LEN: usize = 132;

/// Upper sanity bound for the MOBI header length.
const MAX_MOBI_HEADER_LEN: u32 = 500;

/// `exth_flags` bit signalling an EXTH block follows the MOBI header.
const EXTH_PRESENT_BIT: u32 = 0x40;

/// Sentinel meaning "no image records".
const NO_IMAGE: u32 = 0xFFFF_FFFF;

/// Parsed record 0.
#[derive(Debug)]
pub struct Header {
    /// PalmDOC compression type (informational only).
    #[allow(dead_code)]
    pub compression: u16,
    /// PalmDOC encryption type (recorded, never rejected).
    #[allow(dead_code)]
    pub encryption_type: u16,
    /// MOBI `text_encoding` (codepage, e.g. 65001 for UTF-8).
    pub codepage: u32,
    /// MOBI `file_version` (6 vs 8; informational only).
    #[allow(dead_code)]
    pub file_version: u32,
    /// Raw `full_name` title bytes, when in range.
    pub full_name: Option<Vec<u8>>,
    /// MOBI locale identifier.
    pub locale: u32,
    /// Index of the first image record, when in range.
    pub first_image_index: Option<u32>,
    /// EXTH items in file order: `(record type, raw data)`.
    pub exth: Vec<(u32, Vec<u8>)>,
}

fn u16_be_at(record: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes([record[offset], record[offset + 1]])
}

fn u32_be_at(record: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        record[offset],
        record[offset + 1],
        record[offset + 2],
        record[offset + 3],
    ])
}

/// Parse record 0 given the container's record count `nrec`.
pub fn parse(record0: &[u8], nrec: usize) -> Result<Header> {
    if record0.len() < MIN_RECORD0_LEN {
        return Err(MobiError::Malformed(format!(
            "record 0 too short for MOBI header: {} bytes",
            record0.len()
        )));
    }

    let compression = u16_be_at(record0, 0);
    // u16 at 8 (`record_count`) is deliberately ignored: it excludes image
    // and index records, so the PDB `nrec` owns all slicing.
    let encryption_type = u16_be_at(record0, 12);

    if &record0[16..20] != b"MOBI" {
        return Err(MobiError::NotMobi);
    }
    let header_length = u32_be_at(record0, 20);
    if header_length == 0 || header_length > MAX_MOBI_HEADER_LEN {
        return Err(MobiError::Malformed(format!(
            "implausible MOBI header length: {header_length}"
        )));
    }
    let exth_start = 16usize
        .checked_add(header_length as usize)
        .ok_or_else(|| MobiError::Malformed("MOBI header length overflows".to_string()))?;
    if exth_start > record0.len() {
        return Err(MobiError::Malformed(format!(
            "MOBI header length {header_length} exceeds record 0 length {}",
            record0.len()
        )));
    }

    let codepage = u32_be_at(record0, 28);
    let file_version = u32_be_at(record0, 36);

    let full_name_offset = u32_be_at(record0, 84) as usize;
    let full_name_length = u32_be_at(record0, 88) as usize;
    let full_name = full_name_offset
        .checked_add(full_name_length)
        .filter(|end| *end <= record0.len())
        .map(|end| record0[full_name_offset..end].to_vec());

    let locale = u32_be_at(record0, 92);

    let raw_fii = u32_be_at(record0, 108);
    let first_image_index = if raw_fii == NO_IMAGE || (raw_fii as usize) >= nrec {
        None
    } else {
        Some(raw_fii)
    };

    let exth_flags = u32_be_at(record0, 128);
    let exth = if exth_flags & EXTH_PRESENT_BIT != 0 {
        parse_exth(&record0[exth_start..])
    } else {
        Vec::new()
    };

    Ok(Header {
        compression,
        encryption_type,
        codepage,
        file_version,
        full_name,
        locale,
        first_image_index,
        exth,
    })
}

/// Parse the EXTH block at the start of `block`.
///
/// A magic mismatch (or a block too short for even the fixed header) means
/// EXTH is treated as absent. The declared total length is only a sanity
/// bound: parsing runs to the end of the buffer or the declared item count,
/// whichever comes first. Every item read is bounds-checked; a malformed
/// item is skipped while later valid ones survive. Trailing padding is
/// tolerated.
fn parse_exth(block: &[u8]) -> Vec<(u32, Vec<u8>)> {
    if block.len() < 12 || &block[0..4] != b"EXTH" {
        return Vec::new();
    }
    let declared_len = u32::from_be_bytes([block[4], block[5], block[6], block[7]]) as usize;
    let count = u32::from_be_bytes([block[8], block[9], block[10], block[11]]) as usize;

    let block_end = if declared_len >= 12 {
        declared_len.min(block.len())
    } else {
        block.len()
    };

    let mut items = Vec::new();
    let mut cursor = 12usize;
    for _ in 0..count {
        let Some(remaining) = block_end.checked_sub(cursor) else {
            break;
        };
        if remaining < 8 {
            break;
        }
        let record_type = u32::from_be_bytes([
            block[cursor],
            block[cursor + 1],
            block[cursor + 2],
            block[cursor + 3],
        ]);
        let size = u32::from_be_bytes([
            block[cursor + 4],
            block[cursor + 5],
            block[cursor + 6],
            block[cursor + 7],
        ]) as usize;
        if size < 8 {
            // Corrupt size: skip just this item header and try to resync.
            cursor += 8;
            continue;
        }
        if size > remaining {
            // Item runs past the buffer end: nothing more to parse.
            break;
        }
        items.push((record_type, block[cursor + 8..cursor + size].to_vec()));
        cursor += size;
    }
    items
}
