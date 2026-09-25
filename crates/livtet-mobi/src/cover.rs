//! Cover resolution against the image record index.
//!
//! The MOBI header's `first_image_index` anchors image numbering; EXTH 201
//! (cover) and 202 (thumbnail) give offsets relative to it. Candidates are
//! tried in order — 201, 202, then a forward scan for the first record with
//! image magic — with every index bounds-checked. Non-image records (fonts,
//! FDST, BOUN, HUFF tables, text) are skipped. Returned bytes are raw; no
//! JPEG normalization is applied.

use livtet_importer_types::Cover;

use crate::Result;
use crate::header::Header;
use crate::metadata::{EXTH_COVER_OFFSET, EXTH_THUMB_OFFSET, image_offset};

/// Resolve the cover image, if any.
pub(crate) fn resolve(records: &[&[u8]], header: &Header) -> Result<Option<Cover>> {
    let Some(fii) = header.first_image_index else {
        return Ok(None);
    };
    let nrec = records.len();

    let mut candidates: Vec<usize> = Vec::new();
    for record_type in [EXTH_COVER_OFFSET, EXTH_THUMB_OFFSET] {
        if let Some(offset) = image_offset(header, record_type)
            && let Some(index) = (fii as usize).checked_add(offset as usize)
        {
            candidates.push(index);
        }
    }
    for index in fii as usize..nrec {
        if !candidates.contains(&index) {
            candidates.push(index);
        }
    }

    for index in candidates {
        let Some(record) = records.get(index) else {
            continue;
        };
        if let Some(mime) = image_mime(record) {
            return Ok(Some(Cover {
                data: record.to_vec(),
                mime: mime.to_string(),
            }));
        }
    }
    Ok(None)
}

/// Image magic sniffing; `None` for text, fonts, tables, and other records.
fn image_mime(record: &[u8]) -> Option<&'static str> {
    if record.len() >= 3 && record[..3] == [0xFF, 0xD8, 0xFF] {
        Some("image/jpeg")
    } else if record.len() >= 4 && record[..4] == [0x89, b'P', b'N', b'G'] {
        Some("image/png")
    } else if record.len() >= 6 && (&record[..6] == b"GIF87a" || &record[..6] == b"GIF89a") {
        Some("image/gif")
    } else if record.len() >= 2 && &record[..2] == b"BM" {
        Some("image/bmp")
    } else if record.len() >= 12 && &record[..4] == b"RIFF" && &record[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}
