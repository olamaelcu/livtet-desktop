//! Bibliographic metadata extracted from an EPUB's OPF package document.

use std::collections::BTreeSet;

use livtet_types::Isbn;

use crate::cover::{self, Cover};
use crate::encryption::Encryption;
use crate::error::EpubError;
use crate::ocf;
use crate::xml::{self, Element};
use crate::zip::Archive;

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
    fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "aut" | "author" => Self::Author,
            "edt" | "editor" => Self::Editor,
            "trl" | "translator" => Self::Translator,
            "ill" | "illustrator" => Self::Illustrator,
            other => Self::Other(other.to_string()),
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
    /// Sortable title, from the main title's `file-as` refinement or
    /// `calibre:title_sort`.
    pub title_sort: Option<String>,
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

/// Which DCMES element a contributor came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Creator,
    Contributor,
}

pub(crate) fn extract(archive: &mut Archive) -> Result<EpubMetadata, EpubError> {
    let opf_path = ocf::find_opf_path(archive)?;
    let opf_bytes = archive.read(&opf_path).ok_or_else(|| {
        EpubError::Container(format!(
            "OPF document {opf_path} is not present in the archive"
        ))
    })?;
    let package = xml::parse(&opf_bytes)?;
    let opf_dir = ocf::base_dir(&opf_path);
    let metadata = package.find("metadata").unwrap_or(&package);
    let epub3 = is_epub3(&package);

    let title = extract_title(metadata, epub3)?;
    let title_sort = extract_title_sort(metadata);
    let creators = extract_contributors(metadata)?;
    let (isbns, other_identifiers) = extract_identifiers(metadata)?;
    let encryption = Encryption::load(archive);
    let cover = cover::extract(archive, &package, &opf_dir, &encryption);

    Ok(EpubMetadata {
        title,
        title_sort,
        creators,
        isbns,
        other_identifiers,
        publisher: first_value(metadata, "publisher").map(Publisher),
        language: first_value(metadata, "language").map(Language),
        published: extract_date(metadata, epub3),
        description: first_value(metadata, "description").map(Description),
        subjects: extract_subjects(metadata),
        cover,
    })
}

fn is_epub3(package: &Element) -> bool {
    package
        .attr("version")
        .and_then(|version| {
            version
                .trim()
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|major| major.parse::<u32>().ok())
        })
        .is_some_and(|major| major >= 3)
}

/// Non-empty `<dc:title>` elements in document order.
fn titles(metadata: &Element) -> Vec<&Element> {
    metadata
        .find_all("title")
        .into_iter()
        .filter(|title| !collapse(&title.text()).is_empty())
        .collect()
}

/// Title resolution. EPUB3 prefers the `title-type == main` refinement and
/// appends a subtitle when one is marked; EPUB2 uses the first non-empty title.
fn extract_title(metadata: &Element, epub3: bool) -> Result<Title, EpubError> {
    let titles = titles(metadata);

    let first = titles
        .first()
        .copied()
        .ok_or(EpubError::MissingRequired("title"))?;

    if !epub3 {
        return Ok(Title(collapse(&first.text())));
    }

    let main = main_title(metadata).unwrap_or(first);
    let mut value = collapse(&main.text());
    if let Some(subtitle) = subtitle_title(metadata, main) {
        let subtitle = collapse(&subtitle.text());
        if !subtitle.is_empty() {
            value = format!("{value}: {subtitle}");
        }
    }
    Ok(Title(value))
}

fn main_title(metadata: &Element) -> Option<&Element> {
    let titles = titles(metadata);
    titles
        .iter()
        .find(|title| {
            title_type(metadata, title).is_some_and(|kind| kind.eq_ignore_ascii_case("main"))
        })
        .copied()
        .or_else(|| titles.first().copied())
}

fn subtitle_title<'a>(metadata: &'a Element, main: &Element) -> Option<&'a Element> {
    metadata
        .find_all("title")
        .into_iter()
        .filter(|title| !std::ptr::eq(*title, main))
        .find(|title| {
            title_type(metadata, title).is_some_and(|kind| {
                let lower = kind.to_ascii_lowercase();
                lower.contains("subtitle") || lower.contains("sub-title")
            })
        })
}

fn title_type(metadata: &Element, title: &Element) -> Option<String> {
    refinement(metadata, title, "title-type")
}

fn extract_title_sort(metadata: &Element) -> Option<String> {
    if let Some(main) = main_title(metadata) {
        let file_as = refinement(metadata, main, "file-as")
            .or_else(|| raw_attr(main, "file-as"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if file_as.is_some() {
            return file_as;
        }
    }
    for meta in metadata.find_all("meta") {
        let property = meta.attr("property");
        let name = meta.attr("name");
        if property.is_some_and(|p| p.eq_ignore_ascii_case("calibre:title_sort")) {
            let value = meta.text().trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
        if name.is_some_and(|n| n.eq_ignore_ascii_case("calibre:title_sort"))
            && let Some(content) = meta.attr("content")
        {
            let value = content.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Build the contributor list from `dc:creator` and `dc:contributor` in
/// document order, resolving roles, dropping redundant re-listings, and
/// de-duplicating by `(name, role)`.
fn extract_contributors(metadata: &Element) -> Result<Vec<Contributor>, EpubError> {
    let mut raw: Vec<RawContributor> = Vec::new();
    for element in metadata.descendants() {
        let kind = match element.local.as_str() {
            "creator" => Kind::Creator,
            "contributor" => Kind::Contributor,
            _ => continue,
        };
        let name = collapse(&element.text());
        if name.is_empty() {
            continue;
        }
        let (role, explicit_role) = resolve_role(metadata, element, kind);
        let file_as = refinement(metadata, element, "file-as")
            .or_else(|| raw_attr(element, "file-as"))
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let display_seq = refinement(metadata, element, "display-seq")
            .and_then(|value| value.trim().parse::<i64>().ok())
            .unwrap_or(0);
        raw.push(RawContributor {
            name,
            role,
            explicit_role,
            file_as,
            display_seq,
            kind,
        });
    }

    // A role-less contributor that merely re-lists a creator is redundant and
    // must not produce a second, conflicting author row downstream.
    let creator_names: BTreeSet<String> = raw
        .iter()
        .filter(|entry| entry.kind == Kind::Creator)
        .map(|entry| entry.name.clone())
        .collect();
    raw.retain(|entry| {
        !(entry.kind == Kind::Contributor
            && !entry.explicit_role
            && creator_names.contains(entry.name.as_str()))
    });

    // Stable sort by display-seq keeps document order for equal keys.
    raw.sort_by_key(|entry| entry.display_seq);

    let mut out: Vec<Contributor> = Vec::new();
    for entry in raw {
        if let Some(existing) = out
            .iter_mut()
            .find(|c| c.name == entry.name && c.role == entry.role)
        {
            if existing.file_as.is_none() {
                existing.file_as = entry.file_as;
            }
        } else {
            out.push(Contributor {
                name: entry.name,
                role: entry.role,
                file_as: entry.file_as,
            });
        }
    }

    if out.is_empty() {
        return Err(EpubError::MissingRequired("creator"));
    }
    Ok(out)
}

struct RawContributor {
    name: String,
    role: Role,
    explicit_role: bool,
    file_as: Option<String>,
    display_seq: i64,
    kind: Kind,
}

fn resolve_role(metadata: &Element, element: &Element, kind: Kind) -> (Role, bool) {
    let code = refinement(metadata, element, "role").or_else(|| raw_attr(element, "role"));
    if let Some(code) = code
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
    {
        return (Role::from_code(&code), true);
    }
    match kind {
        Kind::Creator => (Role::Author, false),
        Kind::Contributor => (Role::Other("ctb".to_string()), false),
    }
}

fn extract_identifiers(metadata: &Element) -> Result<(Vec<Isbn>, Vec<Identifier>), EpubError> {
    let mut isbns: Vec<Isbn> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut other_identifiers = Vec::new();

    // `dc:identifier` first, then `dc:source` (Anna's Archive and friends carry
    // the canonical ISBN in `dc:source`), preserving first-seen order.
    let candidates = metadata
        .find_all("identifier")
        .into_iter()
        .chain(metadata.find_all("source"));
    for element in candidates {
        let value = element.text().trim().to_string();
        if value.is_empty() {
            continue;
        }
        match parse_isbn_candidate(&value) {
            Some(isbn) => {
                if seen.insert(isbn.as_str().to_string()) {
                    isbns.push(isbn);
                }
            }
            None => other_identifiers.push(Identifier(value)),
        }
    }

    if isbns.is_empty() {
        return Err(EpubError::MissingIsbn);
    }
    Ok((isbns, other_identifiers))
}

/// EPUB-specific normalization *before* validation: strip a `urn:` prefix,
/// then validate as-is (`livtet-types` handles `isbn:` prefixes, hyphens, and
/// ISBN-10 upgrade). Only on failure do we assume the Sigil quirk — a letter
/// glued onto a numeric NCName id like `a9781784780609` — and strip leading
/// letters one at a time until a valid ISBN emerges.
fn parse_isbn_candidate(value: &str) -> Option<Isbn> {
    let mut candidate = value.trim().to_string();
    // Byte index 4 need not be a UTF-8 char boundary (e.g. `€é` or CJK ids), so
    // probe with `get` — which yields `None` off a boundary — before draining.
    if candidate
        .get(..4)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("urn:"))
    {
        candidate.drain(..4);
    }
    if let Ok(isbn) = Isbn::parse(&candidate) {
        return Some(isbn);
    }
    while candidate.len() > 13
        && candidate
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
    {
        candidate.remove(0);
        if let Ok(isbn) = Isbn::parse(&candidate) {
            return Some(isbn);
        }
    }
    None
}

fn first_value(metadata: &Element, local: &str) -> Option<String> {
    metadata
        .find_all(local)
        .into_iter()
        .map(|element| element.text().trim().to_string())
        .find(|value| !value.is_empty())
}

fn extract_date(metadata: &Element, epub3: bool) -> Option<PublicationDate> {
    let dates: Vec<PublicationDate> = metadata
        .find_all("date")
        .into_iter()
        .filter_map(|element| PublicationDate::parse(&element.text()))
        .collect();
    if epub3 {
        dates.into_iter().next()
    } else {
        dates
            .into_iter()
            .min_by_key(|date| (date.year, date.month.unwrap_or(0), date.day.unwrap_or(0)))
    }
}

fn extract_subjects(metadata: &Element) -> Vec<Subject> {
    let mut subjects: Vec<Subject> = Vec::new();
    for element in metadata.find_all("subject") {
        for part in element.text().split(',') {
            let part = part.trim();
            if !part.is_empty() && !subjects.iter().any(|subject| subject.0 == part) {
                subjects.push(Subject(part.to_string()));
            }
        }
    }
    subjects
}

/// Find a `<meta property="…">` refinement targeting `element`'s id.
fn refinement(metadata: &Element, element: &Element, property: &str) -> Option<String> {
    let id = element.attr("id")?;
    let target = format!("#{id}");
    metadata
        .find_all("meta")
        .into_iter()
        .find(|meta| {
            meta.attr("refines") == Some(target.as_str())
                && meta
                    .attr("property")
                    .is_some_and(|p| p.eq_ignore_ascii_case(property))
        })
        .map(|meta| meta.text().trim().to_string())
}

/// Read a `file-as` / `role` attribute, whether bare or `opf:`-prefixed.
fn raw_attr(element: &Element, local: &str) -> Option<String> {
    element.attr(local).map(str::to_string)
}

/// Collapse all whitespace runs to single spaces and trim.
fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}
