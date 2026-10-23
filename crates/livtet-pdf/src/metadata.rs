//! Merge the Info dictionary and the XMP packet into one source record.
//!
//! The `/Info` dictionary is the older, near-universal carrier; XMP is the
//! richer, ISO 32000-2 metadata stream. XMP wins where both speak, except for
//! the ISBN recovery sweep, which reads both. Extraction is fail-closed: a
//! missing title or creator is an error, never a partial record. An ISBN is
//! optional and may legitimately be empty.

use std::collections::BTreeSet;

use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, Language, PublicationDate, Role, SourceMetadata,
    Subject, Title,
};
use livtet_types::Isbn;
use pdf_oxide::editor::DocumentInfo;
use pdf_oxide::extractors::xmp::XmpMetadata;

use crate::error::PdfError;

/// The bibliographic record extracted from a PDF.
pub type PdfMetadata = SourceMetadata;

/// Combine already-extracted Info, XMP, and cover data into a source record.
///
/// Merge order is XMP → Info for title, creators, language, description, and
/// date; identifiers and subjects are swept from both. `publisher` is always
/// [`None`]: PDF has no standard publisher field, and `pdf_oxide` exposes
/// none in either carrier.
pub(crate) fn merge(
    info: DocumentInfo,
    xmp: Option<XmpMetadata>,
    cover: Option<Cover>,
) -> Result<PdfMetadata, PdfError> {
    let title = extract_title(&info, xmp.as_ref())?;
    let creators = extract_creators(&info, xmp.as_ref())?;
    let (isbns, other_identifiers) = extract_identifiers(&info, xmp.as_ref());
    let subjects = extract_subjects(&info, xmp.as_ref());
    let published = xmp
        .as_ref()
        .and_then(|xmp| xmp.xmp_create_date.as_deref())
        .and_then(parse_pdf_date)
        .or_else(|| info.creation_date.as_deref().and_then(parse_pdf_date));

    Ok(SourceMetadata {
        title,
        title_sort: None,
        creators,
        isbns,
        other_identifiers,
        publisher: None,
        language: xmp
            .as_ref()
            .and_then(|xmp| non_empty(xmp.dc_language.as_deref()))
            .map(|language| Language(language.to_string())),
        published,
        description: xmp
            .as_ref()
            .and_then(|xmp| non_empty(xmp.dc_description.as_deref()))
            .map(|description| Description(description.to_string())),
        subjects,
        cover,
        // PDF carries no per-format metadata: there is no page-count or
        // structure schema for the format, so this stays empty like EPUB/MOBI.
        format_metadata: None,
    })
}

/// Title resolution: XMP `dc:title` first, then the Info `/Title`.
fn extract_title(info: &DocumentInfo, xmp: Option<&XmpMetadata>) -> Result<Title, PdfError> {
    let value = xmp
        .and_then(|xmp| xmp.dc_title.as_deref())
        .map(collapse)
        .filter(|title| !title.is_empty())
        .or_else(|| info.title.as_deref().map(collapse))
        .unwrap_or_default();
    if value.is_empty() {
        return Err(PdfError::MissingRequired("title"));
    }
    Ok(Title(value))
}

/// Contributor resolution: XMP `dc:creator` first, then the Info `/Author`,
/// split on `and`/`with`/`&` separators.
fn extract_creators(
    info: &DocumentInfo,
    xmp: Option<&XmpMetadata>,
) -> Result<Vec<Contributor>, PdfError> {
    let mut names: Vec<String> = Vec::new();
    if let Some(xmp) = xmp {
        for creator in &xmp.dc_creator {
            let name = collapse(creator);
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    if names.is_empty()
        && let Some(author) = info.author.as_deref()
    {
        for name in split_creators(author) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    if names.is_empty() {
        return Err(PdfError::MissingRequired("creator"));
    }
    Ok(names
        .into_iter()
        .map(|name| Contributor {
            name,
            role: Role::Author,
            file_as: None,
        })
        .collect())
}

/// Split a single author string into names.
///
/// Separators are ` and `, ` with `, and ` & `. A book with `;`-separated
/// names is left whole (the semicolon is not a reliable PDF separator), and
/// `&&` is treated as an escaped literal `&` rather than a separator.
fn split_creators(raw: &str) -> Vec<String> {
    const ESCAPED_AMPERSAND: char = '\u{0}';
    let protected = raw.replace("&&", &ESCAPED_AMPERSAND.to_string());
    protected
        .split(" and ")
        .flat_map(|part| part.split(" with "))
        .flat_map(|part| part.split(" & "))
        .map(|part| collapse(&part.replace(ESCAPED_AMPERSAND, "&")))
        .filter(|name| !name.is_empty())
        .collect()
}

/// Every raw string that may carry an identifier, in first-seen order:
/// Info `/Keywords` and `/Subject`, then XMP `pdf:Keywords`, `dc:subject`,
/// and custom values. Each value is comma-split.
fn identifier_candidates<'a>(info: &'a DocumentInfo, xmp: Option<&'a XmpMetadata>) -> Vec<&'a str> {
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(keywords) = info.keywords.as_deref() {
        candidates.extend(keywords.split(','));
    }
    if let Some(subject) = info.subject.as_deref() {
        candidates.extend(subject.split(','));
    }
    if let Some(xmp) = xmp {
        if let Some(keywords) = xmp.pdf_keywords.as_deref() {
            candidates.extend(keywords.split(','));
        }
        candidates.extend(xmp.dc_subject.iter().flat_map(|value| value.split(',')));
        candidates.extend(xmp.custom.values().flat_map(|value| value.split(',')));
    }
    candidates
}

/// Split identifier candidates into validated ISBNs and everything else,
/// de-duplicated in first-seen order. A candidate that is not an ISBN is still
/// preserved as an [`Identifier`], so an edition without an ISBN keeps identity.
fn extract_identifiers(
    info: &DocumentInfo,
    xmp: Option<&XmpMetadata>,
) -> (Vec<Isbn>, Vec<Identifier>) {
    let mut isbns: Vec<Isbn> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut other_identifiers: Vec<Identifier> = Vec::new();
    for candidate in identifier_candidates(info, xmp) {
        let value = candidate.trim();
        if value.is_empty() {
            continue;
        }
        match Isbn::parse(value) {
            Ok(isbn) => {
                if seen.insert(isbn.as_str().to_string()) {
                    isbns.push(isbn);
                }
            }
            Err(_) => {
                if !other_identifiers.iter().any(|known| known.0 == value) {
                    other_identifiers.push(Identifier(value.to_string()));
                }
            }
        }
    }
    (isbns, other_identifiers)
}

/// Subject keywords from XMP `dc:subject` and the comma-split Info
/// `/Subject` / `/Keywords`, excluding anything that validated as an ISBN.
fn extract_subjects(info: &DocumentInfo, xmp: Option<&XmpMetadata>) -> Vec<Subject> {
    let mut subjects: Vec<Subject> = Vec::new();
    let mut push = |value: &str| {
        let value = value.trim();
        if value.is_empty() || Isbn::parse(value).is_ok() {
            return;
        }
        if !subjects.iter().any(|known| known.0 == value) {
            subjects.push(Subject(value.to_string()));
        }
    };
    if let Some(xmp) = xmp {
        for subject in &xmp.dc_subject {
            for part in subject.split(',') {
                push(part);
            }
        }
    }
    if let Some(subject) = info.subject.as_deref() {
        for part in subject.split(',') {
            push(part);
        }
    }
    if let Some(keywords) = info.keywords.as_deref() {
        for part in keywords.split(',') {
            push(part);
        }
    }
    subjects
}

/// Parse a PDF date string into a publication date.
///
/// Handles both the compact PDF form (`D:YYYYMMDDHHmmSS…`, with the `D:`
/// prefix optional) and the ISO 8601 form XMP carries (`YYYY-MM-DD`).
/// Resolution beyond `YYYY-MM-DD` is discarded; a malformed value yields
/// [`None`] rather than an error.
fn parse_pdf_date(raw: &str) -> Option<PublicationDate> {
    let trimmed = raw.trim();
    let body = trimmed.strip_prefix("D:").unwrap_or(trimmed);
    let bytes = body.as_bytes();
    let compact =
        bytes.len() >= 5 && bytes[..4].iter().all(u8::is_ascii_digit) && bytes[4].is_ascii_digit();
    if !compact {
        return PublicationDate::parse(body);
    }
    let year: i32 = body.get(0..4)?.parse().ok()?;
    // Clamp year to a sane range to avoid Tantivy datetime overflow on reindex.
    if !(1..=9999).contains(&year) {
        return None;
    }
    let component = |range: std::ops::Range<usize>| {
        body.get(range)
            .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|part| part.parse::<u8>().ok())
    };
    let month = component(4..6).filter(|month| (1..=12).contains(month));
    let day = component(6..8).filter(|day| (1..=31).contains(day));
    Some(PublicationDate { year, month, day })
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Collapse all whitespace runs to single spaces and trim.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::{DocumentInfo, PdfError, XmpMetadata, merge, parse_pdf_date};
    use livtet_importer_types::PublicationDate;

    const ISBN: &str = "9780306406157";

    fn info(title: &str, author: &str) -> DocumentInfo {
        DocumentInfo {
            title: Some(title.to_string()),
            author: Some(author.to_string()),
            ..DocumentInfo::default()
        }
    }

    #[test]
    fn xmp_wins_over_info_for_title_creators_and_date() {
        let info = DocumentInfo {
            creation_date: Some("D:19990101".to_string()),
            ..info("Info Title", "Info Author")
        };
        let xmp = XmpMetadata {
            dc_title: Some("  Xmp \n Title ".to_string()),
            dc_creator: vec![
                "Ann  Lee".to_string(),
                " Ann Lee ".to_string(),
                "   ".to_string(),
                "Bo Kim".to_string(),
            ],
            dc_language: Some(" en ".to_string()),
            dc_description: Some("   ".to_string()),
            xmp_create_date: Some("2020-01-02".to_string()),
            ..XmpMetadata::default()
        };

        let merged = merge(info, Some(xmp), None).unwrap();

        assert_eq!(merged.title.0, "Xmp Title");
        let names: Vec<&str> = merged.creators.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Ann Lee", "Bo Kim"],
            "collapsed, de-duplicated, blanks dropped"
        );
        assert_eq!(merged.language.map(|l| l.0).as_deref(), Some("en"));
        assert_eq!(merged.description, None, "a blank description is absent");
        assert_eq!(merged.published, date(2020, Some(1), Some(2)));
    }

    #[test]
    fn blank_xmp_creators_fall_back_to_info_author() {
        let xmp = XmpMetadata {
            dc_creator: vec!["  ".to_string()],
            ..XmpMetadata::default()
        };
        let merged = merge(info("T", "Cy Dee & Eve Fox"), Some(xmp), None).unwrap();
        let names: Vec<&str> = merged.creators.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Cy Dee", "Eve Fox"]);
    }

    #[test]
    fn unparseable_xmp_date_falls_back_to_info_creation_date() {
        let info = DocumentInfo {
            creation_date: Some("D:20110304".to_string()),
            ..info("T", "A")
        };
        let xmp = XmpMetadata {
            xmp_create_date: Some("someday".to_string()),
            ..XmpMetadata::default()
        };
        let merged = merge(info, Some(xmp), None).unwrap();
        assert_eq!(merged.published, date(2011, Some(3), Some(4)));
    }

    #[test]
    fn identifiers_and_subjects_are_swept_from_info_and_xmp() {
        let info = DocumentInfo {
            keywords: Some(format!("{ISBN}, foo, ,")),
            subject: Some(format!("bar,{ISBN}")),
            ..info("T", "A")
        };
        let mut xmp = XmpMetadata {
            pdf_keywords: Some(ISBN.to_string()),
            dc_subject: vec!["baz, foo".to_string()],
            ..XmpMetadata::default()
        };
        xmp.custom
            .insert("doi".to_string(), "doi:10.1/x".to_string());

        let merged = merge(info, Some(xmp), None).unwrap();

        let isbns: Vec<&str> = merged.isbns.iter().map(|isbn| isbn.as_str()).collect();
        assert_eq!(isbns, [ISBN], "the ISBN is recovered once across carriers");
        let others: Vec<&str> = merged
            .other_identifiers
            .iter()
            .map(|id| id.0.as_str())
            .collect();
        assert_eq!(others, ["foo", "bar", "baz", "doi:10.1/x"]);
        let subjects: Vec<&str> = merged.subjects.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(subjects, ["baz", "foo", "bar"], "XMP first, ISBNs excluded");
    }

    #[test]
    fn blank_xmp_title_falls_back_to_info_title() {
        let xmp = XmpMetadata {
            dc_title: Some("   ".to_string()),
            ..XmpMetadata::default()
        };
        let merged = merge(info("Info Title", "A"), Some(xmp), None).unwrap();
        assert_eq!(merged.title.0, "Info Title");
    }

    #[test]
    fn blank_title_everywhere_fails_closed() {
        let error = merge(info("  ", "A"), None, None).unwrap_err();
        assert!(
            matches!(error, PdfError::MissingRequired("title")),
            "{error:?}"
        );
    }

    #[test]
    fn year_zero_compact_date_is_rejected() {
        assert_eq!(parse_pdf_date("D:00000101"), None);
    }

    fn date(year: i32, month: Option<u8>, day: Option<u8>) -> Option<PublicationDate> {
        Some(PublicationDate { year, month, day })
    }

    #[test]
    fn parses_compact_pdf_dates() {
        assert_eq!(parse_pdf_date("D:2026"), date(2026, None, None));
        assert_eq!(parse_pdf_date("D:202604"), date(2026, Some(4), None));
        assert_eq!(
            parse_pdf_date("D:20260421120000Z"),
            date(2026, Some(4), Some(21))
        );
        assert_eq!(parse_pdf_date("20250819"), date(2025, Some(8), Some(19)));
    }

    #[test]
    fn parses_iso_dates() {
        assert_eq!(parse_pdf_date("2025"), date(2025, None, None));
        assert_eq!(parse_pdf_date("2025-08"), date(2025, Some(8), None));
        assert_eq!(
            parse_pdf_date("2025-08-19T12:00:00Z"),
            date(2025, Some(8), Some(19))
        );
    }

    #[test]
    fn rejects_out_of_range_and_malformed_dates() {
        assert_eq!(parse_pdf_date("D:20261340"), date(2026, None, None));
        assert_eq!(parse_pdf_date(""), None);
        assert_eq!(parse_pdf_date("not a date"), None);
    }
}
