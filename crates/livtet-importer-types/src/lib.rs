//! Shared parser-side metadata model for Livtet file importers.
//!
//! Parser crates (EPUB, AZW3, …) return [`SourceMetadata`]; `livtet-importer`
//! maps it onto the Lua wire contract.

pub use livtet_types::Isbn;

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
    Narrator,
    Other(String),
}

impl Role {
    /// Pure MARC relator code mapping (`aut`/`author` → [`Role::Author`], …);
    /// no EPUB-specific dependencies.
    pub fn from_code(code: &str) -> Self {
        match code.trim().to_ascii_lowercase().as_str() {
            "aut" | "author" => Self::Author,
            "edt" | "editor" => Self::Editor,
            "trl" | "translator" => Self::Translator,
            "ill" | "illustrator" => Self::Illustrator,
            "nrt" | "narrator" => Self::Narrator,
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
    pub fn parse(raw: &str) -> Option<Self> {
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

/// Front-cover image bytes and media type as declared in the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cover {
    pub data: Vec<u8>,
    pub mime: String,
}

/// Complete bibliographic record extracted from a source file, guaranteed to
/// carry at least a title and one creator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMetadata {
    pub title: Title,
    /// Sortable title, from the main title's `file-as` refinement or
    /// `calibre:title_sort`.
    pub title_sort: Option<String>,
    pub creators: Vec<Contributor>,
    /// Validated ISBN-13s found in the metadata or recovered from the content
    /// documents; may be empty when no ISBN can be found.
    pub isbns: Vec<Isbn>,
    /// Identifier strings that are not valid ISBNs.
    pub other_identifiers: Vec<Identifier>,
    pub publisher: Option<Publisher>,
    pub language: Option<Language>,
    pub published: Option<PublicationDate>,
    pub description: Option<Description>,
    pub subjects: Vec<Subject>,
    pub cover: Option<Cover>,
    /// Optional per-edition format metadata (e.g. audiobook duration and
    /// chapters), carried opaquely and validated against the edition format's
    /// `FormatMetadataSchema` at import time.
    pub format_metadata: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::{Role, SourceMetadata, Title};

    #[test]
    fn narrator_role_from_code() {
        assert_eq!(Role::from_code("nrt"), Role::Narrator);
        assert_eq!(Role::from_code("narrator"), Role::Narrator);
        assert_eq!(Role::from_code("NRT"), Role::Narrator);
    }

    #[test]
    fn source_metadata_carries_optional_format_metadata() {
        let meta = SourceMetadata {
            title: Title("T".to_string()),
            title_sort: None,
            creators: Vec::new(),
            isbns: Vec::new(),
            other_identifiers: Vec::new(),
            publisher: None,
            language: None,
            published: None,
            description: None,
            subjects: Vec::new(),
            cover: None,
            format_metadata: None,
        };
        assert!(meta.format_metadata.is_none());
    }
}

/// Split a raw contributor string into individual names on `;` and on the
/// standalone word `and` (ASCII case-insensitive).
///
/// Each part is whitespace-collapsed and trimmed, and empty parts are dropped;
/// a string with no separator yields a single element. The word `and` only
/// splits when it is a whole token, so `Alexander Anderson` stays intact, and
/// `&` is never a separator.
pub fn split_contributor_name(raw: &str) -> Vec<String> {
    let mut names = Vec::new();
    for segment in raw.split(';') {
        let mut current: Vec<&str> = Vec::new();
        for word in segment.split_whitespace() {
            if word.eq_ignore_ascii_case("and") {
                push_name(&mut names, &current);
                current.clear();
            } else {
                current.push(word);
            }
        }
        push_name(&mut names, &current);
    }
    names
}

fn push_name(out: &mut Vec<String>, words: &[&str]) {
    let name = words.join(" ");
    if !name.is_empty() {
        out.push(name);
    }
}

#[cfg(test)]
mod tests {
    use super::split_contributor_name;

    #[test]
    fn splits_on_standalone_and() {
        assert_eq!(
            split_contributor_name("John Smith and Jane Doe"),
            vec!["John Smith", "Jane Doe"]
        );
    }

    #[test]
    fn splits_on_semicolon() {
        assert_eq!(
            split_contributor_name("Smith, John; Doe, Jane"),
            vec!["Smith, John", "Doe, Jane"]
        );
    }

    #[test]
    fn and_is_case_insensitive() {
        assert_eq!(split_contributor_name("A AND B"), vec!["A", "B"]);
    }

    #[test]
    fn and_inside_a_word_is_not_a_separator() {
        assert_eq!(
            split_contributor_name("Alexander Anderson"),
            vec!["Alexander Anderson"]
        );
    }

    #[test]
    fn ampersand_is_not_a_separator() {
        assert_eq!(split_contributor_name("Mutts & Co."), vec!["Mutts & Co."]);
    }

    #[test]
    fn collapses_repeated_and_empty_separators() {
        assert_eq!(split_contributor_name("A and B and C"), vec!["A", "B", "C"]);
        assert_eq!(split_contributor_name("  and A ;; and "), vec!["A"]);
    }
}
