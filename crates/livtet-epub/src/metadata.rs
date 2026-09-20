//! Bibliographic metadata extracted from an EPUB's OPF package document.

use epub::doc::EpubDoc;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::BufReader;

use crate::cover::{self, Cover};
use crate::error::EpubError;
use livtet_types::Isbn;

/// The main title of the publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(pub String);

/// A person or organization credited with the work (DCMES `creator` or
/// `contributor`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contributor {
    pub name: String,
    pub role: Role,
    /// Sortable form of the name (`file-as` refinement), when present.
    pub file_as: Option<String>,
}

/// MARC relator–style contributor role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Role {
    Author,
    Editor,
    Translator,
    Illustrator,
    Other(String),
}

impl Role {
    fn from_refinement(value: Option<&str>) -> Self {
        match value.map(str::trim).map(str::to_ascii_lowercase) {
            None => Self::Author,
            Some(code) => match code.as_str() {
                "aut" | "author" => Self::Author,
                "edt" | "editor" => Self::Editor,
                "trl" | "translator" => Self::Translator,
                "ill" | "illustrator" => Self::Illustrator,
                other => Self::Other(other.to_string()),
            },
        }
    }
}

/// A non-ISBN identifier preserved verbatim (DOI, publisher id, …).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identifier(pub String);

/// Primary language tag of the publication (BCP-47, e.g. `en`, `en-US`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Language(pub String);

/// Publisher name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Publisher(pub String);

/// Long-form description / synopsis (HTML allowed by the spec).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description(pub String);

/// Subject keyword (DDC/LCSH-relevant cataloging happens elsewhere).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject(pub String);

/// Publication date, with the precision the file actually carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationDate {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl PublicationDate {
    /// Accept `YYYY`, `YYYY-MM`, or `YYYY-MM-DD` (trailing time parts are
    /// dropped).
    fn parse(raw: &str) -> Option<Self> {
        let mut it = raw.trim().split(['-', 'T', ' ']);
        let year: i32 = it.next()?.parse().ok()?;
        let month = it
            .next()
            .and_then(|m| m.parse().ok())
            .filter(|m| (1..=12).contains(m));
        let day = it
            .next()
            .and_then(|d| d.parse().ok())
            .filter(|d| (1..=31).contains(d));
        Some(Self { year, month, day })
    }
}

/// Complete bibliographic record extracted from an EPUB, guaranteed to carry
/// at least a title and one valid ISBN.
#[derive(Debug)]
pub struct EpubMetadata {
    pub title: Title,
    pub creators: Vec<Contributor>,
    /// Validated ISBN-13s found anywhere in the metadata; never empty.
    pub isbns: Vec<Isbn>,
    /// Identifier strings that are not valid ISBNs.
    pub other_identifiers: Vec<Identifier>,
    pub publisher: Option<Publisher>,
    pub language: Option<Language>,
    pub published: Option<PublicationDate>,
    pub description: Option<Description>,
    pub subjects: Vec<Subject>,
    pub cover: Option<Cover>,
}

pub(crate) fn extract(doc: &mut EpubDoc<BufReader<File>>) -> Result<EpubMetadata, EpubError> {
    let title = doc
        .get_title()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .map(Title)
        .ok_or(EpubError::MissingRequired("title"))?;

    let creators = contributors(doc);
    if creators.is_empty() {
        return Err(EpubError::MissingRequired("creator"));
    }

    let mut isbns: Vec<Isbn> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut other_identifiers = Vec::new();
    for candidate in identifier_candidates(doc) {
        // EPUB-specific candidates (urn:/Sigil quirks), then canonical
        // validation from livtet-types.
        match parse_isbn_candidate(&candidate) {
            Some(isbn) => {
                if seen.insert(isbn.as_str().to_string()) {
                    isbns.push(isbn);
                }
            }
            None => other_identifiers.push(Identifier(candidate)),
        }
    }
    if isbns.is_empty() {
        return Err(EpubError::MissingIsbn);
    }

    Ok(EpubMetadata {
        title,
        creators,
        isbns,
        other_identifiers,
        publisher: first_value(doc, "publisher").map(Publisher),
        language: first_value(doc, "language").map(Language),
        published: first_value(doc, "date").and_then(|d| PublicationDate::parse(&d)),
        description: first_value(doc, "description").map(Description),
        subjects: doc
            .metadata
            .iter()
            .filter(|m| m.property == "subject")
            .map(|m| m.value.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(Subject)
            .collect(),
        cover: cover::extract(doc),
    })
}

/// Duplicate titles/roles are dropped; order is preserved.
fn contributors(doc: &EpubDoc<BufReader<File>>) -> Vec<Contributor> {
    let mut out = Vec::new();
    for item in doc
        .metadata
        .iter()
        .filter(|m| m.property == "creator" || m.property == "contributor")
    {
        let name = item.value.trim().to_string();
        if name.is_empty() {
            continue;
        }
        let role = Role::from_refinement(item.refinement("role").map(|r| r.value.as_str()));
        let file_as = item.refinement("file-as").map(|r| r.value.clone());
        let c = Contributor {
            name,
            role,
            file_as,
        };
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

/// Every string that might be an ISBN: `dc:identifier` and `dc:source`
/// (e.g. Anna's Archive carries the ISBN in `dc:source`). Values are
/// pre-cleaned, then validated by [`Isbn::parse`] — see [`parse_isbn_candidate`].
fn identifier_candidates(doc: &EpubDoc<BufReader<File>>) -> Vec<String> {
    let mut out = Vec::new();
    for m in doc
        .metadata
        .iter()
        .filter(|m| m.property == "identifier" || m.property == "source")
    {
        let v = m.value.trim().to_string();
        if !v.is_empty() {
            out.push(v);
        }
    }
    for scheme_item in doc
        .metadata
        .iter()
        .filter(|m| m.property == "identifier-scheme")
    {
        let _ = scheme_item; // schemes are hints; parsing is content-driven
    }
    out
}

/// EPUB-specific normalization *before* validation: strip a `urn:` prefix,
/// then validate as-is (livtet-types handles `isbn:` prefixes, hyphens, and
/// ISBN-10 upgrade). Only on failure do we assume the Sigil quirk — a letter
/// glued onto a numeric NCName id like `a9781784780609` — and strip leading
/// letters one at a time until a valid ISBN emerges.
fn parse_isbn_candidate(value: &str) -> Option<Isbn> {
    let mut s = value.trim().to_string();
    if s.len() >= 4 && s[..4].eq_ignore_ascii_case("urn:") {
        s.drain(..4);
    }
    if let Ok(isbn) = Isbn::parse(&s) {
        return Some(isbn);
    }
    let mut candidate = s;
    while candidate.len() > 13
        && candidate
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic())
    {
        candidate.remove(0);
        if let Ok(isbn) = Isbn::parse(&candidate) {
            return Some(isbn);
        }
    }
    None
}

fn first_value(doc: &EpubDoc<BufReader<File>>, property: &str) -> Option<String> {
    doc.metadata
        .iter()
        .find(|m| m.property == property)
        .map(|m| m.value.trim().to_string())
        .filter(|v| !v.is_empty())
}
