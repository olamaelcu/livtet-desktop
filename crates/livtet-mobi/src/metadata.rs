//! Bibliographic extraction: EXTH record map → [`SourceMetadata`].
//!
//! String EXTH values decode with the MOBI `text_encoding` codepage and are
//! trimmed; only `full_name` and EXTH 108 additionally resolve HTML/XML
//! entities. EXTH ids outside the map below are ignored — including the fake
//! cover (203), boundary marker (121), print length (501), title-sort (504),
//! creator-software ranges (525–548), and DRM markers (1/2/3/208/209).

use std::collections::HashSet;

use livtet_importer_types::{
    Contributor, Description, Identifier, Isbn, Language, PublicationDate, Publisher, Role,
    SourceMetadata, Subject, Title,
};

use crate::Result;
use crate::cover;
use crate::encoding::{decode, decode_entities};
use crate::error::MobiError;
use crate::header::{self, Header};
use crate::pdb::Pdb;

/// EXTH record types.
const EXTH_AUTHOR: u32 = 100;
const EXTH_PUBLISHER: u32 = 101;
// EXTH 102 (imprint) is intentionally not mapped: the shared model has a
// single publisher slot owned by EXTH 101.
const EXTH_DESCRIPTION: u32 = 103;
const EXTH_ISBN: u32 = 104;
const EXTH_SUBJECT: u32 = 105;
const EXTH_PUBLISHED: u32 = 106;
const EXTH_CONTRIBUTOR: u32 = 108;
const EXTH_SOURCE_ID: u32 = 112;
const EXTH_ASIN: u32 = 113;
pub(crate) const EXTH_COVER_OFFSET: u32 = 201;
pub(crate) const EXTH_THUMB_OFFSET: u32 = 202;
const EXTH_TITLE: u32 = 503;
const EXTH_LANGUAGE: u32 = 524;

/// Value that never addresses a record.
const NO_OFFSET: u32 = 0xFFFF_FFFF;

/// Extract metadata and cover from a whole MOBI-family file.
pub(crate) fn extract(bytes: &[u8]) -> Result<SourceMetadata> {
    let pdb = Pdb::parse(bytes)?;
    let record0 = pdb.records().first().ok_or(MobiError::Malformed(
        "container holds no records".to_string(),
    ))?;
    let header = header::parse(record0, pdb.len())?;
    let codepage = header.codepage;

    let title = extract_title(&header, codepage)?;
    let creators = extract_creators(&header, codepage)?;
    let (isbns, other_identifiers) = extract_identifiers(&header, codepage);

    Ok(SourceMetadata {
        title,
        title_sort: None,
        creators,
        isbns,
        other_identifiers,
        publisher: extract_publisher(&header, codepage),
        language: extract_language(&header, codepage),
        published: extract_published(&header, codepage),
        description: extract_description(&header, codepage),
        subjects: extract_subjects(&header, codepage),
        cover: cover::resolve(pdb.records(), &header)?,
    })
}

/// Decode + trim every EXTH value with the given record type, in file order.
fn values(header: &Header, record_type: u32, codepage: u32) -> Vec<String> {
    header
        .exth
        .iter()
        .filter(|(id, _)| *id == record_type)
        .map(|(_, data)| decode(data, codepage).trim().to_string())
        .collect()
}

fn extract_title(header: &Header, codepage: u32) -> Result<Title> {
    if let Some(title) = values(header, EXTH_TITLE, codepage)
        .into_iter()
        .find(|v| !v.is_empty())
    {
        return Ok(Title(title));
    }
    match header.full_name.as_deref() {
        Some(bytes) => {
            let title = decode_entities(&decode(bytes, codepage)).trim().to_string();
            if title.is_empty() {
                Err(MobiError::MissingRequired("title"))
            } else {
                Ok(Title(title))
            }
        }
        None => Err(MobiError::MissingRequired("title")),
    }
}

fn extract_creators(header: &Header, codepage: u32) -> Result<Vec<Contributor>> {
    let mut creators = Vec::new();
    for (id, data) in &header.exth {
        match *id {
            EXTH_AUTHOR => {
                let raw = decode(data, codepage).trim().to_string();
                if raw.is_empty() {
                    continue;
                }
                let (name, file_as) = split_last_first(&raw);
                creators.push(Contributor {
                    name,
                    role: Role::Author,
                    file_as,
                });
            }
            EXTH_CONTRIBUTOR => {
                let name = decode_entities(&decode(data, codepage)).trim().to_string();
                if name.is_empty() {
                    continue;
                }
                creators.push(Contributor {
                    name,
                    role: Role::Other("ctb".to_string()),
                    file_as: None,
                });
            }
            _ => {}
        }
    }
    if creators.is_empty() {
        return Err(MobiError::MissingRequired("creator"));
    }
    Ok(creators)
}

/// Split a single clean `Last, First` into `("First Last", Some(original))`.
///
/// Anything else — no comma, more than one comma, or an empty side — stays
/// verbatim with no `file_as`. Raw UTF-8, parentheses, and accents pass
/// through untouched.
fn split_last_first(raw: &str) -> (String, Option<String>) {
    if raw.chars().filter(|&c| c == ',').count() == 1 {
        let mut parts = raw.splitn(2, ',');
        let last = parts.next().unwrap_or("").trim();
        let first = parts.next().unwrap_or("").trim();
        if !last.is_empty() && !first.is_empty() {
            return (format!("{first} {last}"), Some(raw.to_string()));
        }
    }
    (raw.to_string(), None)
}

fn extract_publisher(header: &Header, codepage: u32) -> Option<Publisher> {
    values(header, EXTH_PUBLISHER, codepage)
        .into_iter()
        .find(|v| !v.is_empty() && v != "Unknown")
        .map(Publisher)
}

fn extract_description(header: &Header, codepage: u32) -> Option<Description> {
    header
        .exth
        .iter()
        .filter(|(id, _)| *id == EXTH_DESCRIPTION)
        .map(|(_, data)| decode(data, codepage))
        .find(|v| !v.is_empty())
        .map(Description)
}

fn extract_identifiers(header: &Header, codepage: u32) -> (Vec<Isbn>, Vec<Identifier>) {
    let mut isbns = Vec::new();
    let mut seen = HashSet::new();
    let mut other_identifiers = Vec::new();

    for value in values(header, EXTH_ISBN, codepage) {
        if value.is_empty() {
            continue;
        }
        push_isbn_candidate(
            &value,
            &value,
            &mut isbns,
            &mut seen,
            &mut other_identifiers,
        );
    }

    for value in values(header, EXTH_SOURCE_ID, codepage) {
        if value.is_empty() {
            continue;
        }
        let lower = value.to_ascii_lowercase();
        if let Some(rest) = strip_case_prefix(&value, &lower, "urn:isbn:") {
            // Strip the matched prefix, then validate.
            push_isbn_candidate(rest, &value, &mut isbns, &mut seen, &mut other_identifiers);
        } else if let Some(colon) = lower.find(':')
            && lower[..colon].trim_end() == "ebook isbn"
        {
            let rest = value[colon + 1..].trim();
            push_isbn_candidate(rest, &value, &mut isbns, &mut seen, &mut other_identifiers);
        } else if lower.starts_with("calibre:") {
            // Internal calibre app id, not a book identifier.
        } else {
            other_identifiers.push(Identifier(value));
        }
    }

    for value in values(header, EXTH_ASIN, codepage) {
        if !value.is_empty() && value.is_ascii() {
            other_identifiers.push(Identifier(value));
        }
    }

    (isbns, other_identifiers)
}

/// Validate `raw` as an ISBN, pushing onto `isbns` (first-seen order) or
/// preserving `verbatim` in `other_identifiers` on failure.
fn push_isbn_candidate(
    raw: &str,
    verbatim: &str,
    isbns: &mut Vec<Isbn>,
    seen: &mut HashSet<String>,
    other_identifiers: &mut Vec<Identifier>,
) {
    match Isbn::parse(raw) {
        Ok(isbn) => {
            if seen.insert(isbn.as_str().to_string()) {
                isbns.push(isbn);
            }
        }
        Err(_) => other_identifiers.push(Identifier(verbatim.to_string())),
    }
}

/// Strip an ASCII case-insensitive prefix, returning the remainder with its
/// original casing.
fn strip_case_prefix<'a>(value: &'a str, lower: &str, prefix: &str) -> Option<&'a str> {
    if lower.starts_with(prefix) {
        Some(&value[prefix.len()..])
    } else {
        None
    }
}

fn extract_subjects(header: &Header, codepage: u32) -> Vec<Subject> {
    let mut subjects = Vec::new();
    let mut seen = HashSet::new();
    for value in values(header, EXTH_SUBJECT, codepage) {
        for part in value.split(';') {
            let part = part.trim();
            if !part.is_empty() && seen.insert(part.to_string()) {
                subjects.push(Subject(part.to_string()));
            }
        }
    }
    subjects
}

fn extract_published(header: &Header, codepage: u32) -> Option<PublicationDate> {
    values(header, EXTH_PUBLISHED, codepage)
        .into_iter()
        .find(|v| !v.is_empty())
        .and_then(|v| PublicationDate::parse(&v))
}

fn extract_language(header: &Header, codepage: u32) -> Option<Language> {
    for value in values(header, EXTH_LANGUAGE, codepage) {
        let subtag = value
            .to_lowercase()
            .split(['-', '_', '@'])
            .next()
            .unwrap_or("")
            .to_string();
        if !subtag.is_empty() {
            return Some(Language(subtag));
        }
    }
    // Locale fallback: the low byte is a Windows language id; 9 is English.
    if header.locale & 0xFF == 9 {
        Some(Language("en".to_string()))
    } else {
        None
    }
}

/// First present cover/thumbnail record offset, if well-formed.
pub(crate) fn image_offset(header: &Header, record_type: u32) -> Option<u32> {
    header
        .exth
        .iter()
        .filter(|(id, _)| *id == record_type)
        .filter(|(_, data)| data.len() >= 4)
        .map(|(_, data)| u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
        .find(|offset| *offset != NO_OFFSET)
}
