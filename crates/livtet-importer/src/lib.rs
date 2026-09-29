//! Typed contract for Livtet file importers.
//!
//! The contract is intentionally small: native and Lua-backed importers agree on
//! which extensions they handle and on the bibliographic record they return.
//! Remote callers use [`ImporterMeta`] as the JSON boundary.

mod audio;
mod epub;
mod mobi;
mod pdf;

pub use audio::AudiobookImporter;
pub use epub::EpubImporter;
pub use mobi::MobiImporter;
pub use pdf::PdfImporter;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};
use stanchion_lua::{
    lua_class,
    mlua::{FromLua, Lua, LuaSerdeExt, Result, Table, Value},
};

/// A contributor in importer order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterContributor {
    pub name: String,
    pub role: Option<String>,
    pub file_as: Option<String>,
}

impl FromLua for ImporterContributor {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// Publication date, with the precision the file actually carries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterPublicationDate {
    pub year: i32,
    pub month: Option<u8>,
    pub day: Option<u8>,
}

impl FromLua for ImporterPublicationDate {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// Front-cover image supplied as standard base64, because JSON cannot carry raw bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterCover {
    pub mime: String,
    pub data_base64: String,
}

impl FromLua for ImporterCover {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// Bibliographic record returned by an importer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImporterMeta {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_sort: Option<String>,
    pub contributors: Vec<ImporterContributor>,
    pub isbns: Vec<String>,
    pub other_identifiers: Vec<String>,
    pub publisher: Option<String>,
    pub language: Option<String>,
    pub published: Option<ImporterPublicationDate>,
    pub description: Option<String>,
    pub subjects: Vec<String>,
    pub cover: Option<ImporterCover>,
    /// Optional per-edition format metadata (e.g. audiobook duration and
    /// chapters), carried opaquely and validated against the edition format's
    /// `FormatMetadataSchema` at import time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format_metadata: Option<Json>,
}

fn normalize_optional_object(value: &mut Json) {
    if matches!(value, Json::Object(entries) if entries.is_empty()) {
        *value = Json::Null;
    }
}

fn normalize_optional_fields(record: &mut Map<String, Json>, fields: &[&str]) {
    for field in fields {
        if let Some(value) = record.get_mut(*field) {
            normalize_optional_object(value);
        }
    }
}

fn normalize_contributor_optionals(record: &mut Map<String, Json>) {
    let Some(Json::Array(contributors)) = record.get_mut("contributors") else {
        return;
    };
    for contributor in contributors {
        if let Some(contributor) = contributor.as_object_mut() {
            normalize_optional_fields(contributor, &["role", "file_as"]);
        }
    }
}

fn normalize_publication_optionals(record: &mut Map<String, Json>) {
    let Some(published) = record.get_mut("published") else {
        return;
    };
    if let Some(published) = published.as_object_mut() {
        normalize_optional_fields(published, &["month", "day"]);
    }
}

impl ImporterMeta {
    /// Decode importer metadata after Lua's lossy table-to-JSON boundary.
    ///
    /// An empty Lua table has no way to say whether it is an empty list or an
    /// empty object, and the host sends `{}`. Normalize the known list fields
    /// before deserializing.
    pub fn from_wire_json(mut value: Json) -> std::result::Result<Self, serde_json::Error> {
        if let Some(record) = value.as_object_mut() {
            normalize_optional_fields(
                record,
                &[
                    "title_sort",
                    "publisher",
                    "language",
                    "published",
                    "description",
                    "cover",
                    "format_metadata",
                ],
            );
            normalize_contributor_optionals(record);
            normalize_publication_optionals(record);
            for field in ["contributors", "isbns", "other_identifiers", "subjects"] {
                if let Some(list) = record.get_mut(field)
                    && let Json::Object(entries) = list
                    && entries.is_empty()
                {
                    *list = Json::Array(Vec::new());
                }
            }
        }
        serde_json::from_value(value)
    }
}

impl FromLua for ImporterMeta {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// Importers publish file extensions and extract one record from a file.
#[lua_class]
pub trait Importer {
    /// `Importer.new(config, deps)` — receiverless, so it lands on `ImporterClass`.
    fn new(config: Table, deps: Table) -> Result<Self>;

    /// File extensions, without leading dots, in lowercase.
    fn extensions(&self) -> Result<Vec<String>>;

    /// Extract bibliographic metadata from a file path.
    fn read_metadata(&self, path: String) -> Result<ImporterMeta>;
}

/// One of a book's format files within a source library, relative to its root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookFile {
    /// Path relative to the library root (the application resolves it).
    pub path: String,
    /// Format tag, lowercase and without a leading dot (e.g. `epub`, `pdf`).
    pub format: String,
}

impl FromLua for BookFile {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// One book discovered by a [`LibraryImporter`]: its metadata, its format files,
/// and an optional cover, all with paths relative to the library root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BookRecord {
    pub meta: ImporterMeta,
    #[serde(default)]
    pub files: Vec<BookFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover_path: Option<String>,
}

impl BookRecord {
    /// Decode one record after Lua's lossy table-to-JSON boundary, normalizing
    /// the nested [`ImporterMeta`] the same way [`ImporterMeta::from_wire_json`]
    /// does.
    pub fn from_wire_json(value: Json) -> std::result::Result<Self, serde_json::Error> {
        let mut record = match value {
            Json::Object(record) => record,
            other => return serde_json::from_value(other),
        };
        let meta = ImporterMeta::from_wire_json(record.remove("meta").unwrap_or(Json::Null))?;
        let files = match record.remove("files") {
            Some(Json::Array(items)) => serde_json::from_value(Json::Array(items))?,
            _ => Vec::new(),
        };
        let cover_path = match record.remove("cover_path") {
            Some(Json::String(path)) if !path.is_empty() => Some(path),
            _ => None,
        };
        Ok(Self {
            meta,
            files,
            cover_path,
        })
    }
}

impl FromLua for BookRecord {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        lua.from_value(value)
    }
}

/// A library importer scans a whole source library (e.g. a Calibre library) and
/// yields one [`BookRecord`] per book. Unlike [`Importer`], which reads a single
/// file, it enumerates many; the application persists the records and imports
/// their files.
#[lua_class]
pub trait LibraryImporter {
    /// `LibraryImporter.new(config, deps)` — receiverless, lands on the class.
    fn new(config: Table, deps: Table) -> Result<Self>;

    /// Scan the library rooted at `source`, returning one record per book.
    fn scan(&self, source: String) -> Result<Vec<BookRecord>>;
}

pub(crate) fn role_string(role: &livtet_epub::Role) -> String {
    match role {
        livtet_epub::Role::Author => "aut",
        livtet_epub::Role::Editor => "edt",
        livtet_epub::Role::Translator => "trl",
        livtet_epub::Role::Illustrator => "ill",
        livtet_epub::Role::Narrator => "nrt",
        livtet_epub::Role::Other(raw) => raw,
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_record_normalizes_empty_lua_tables() {
        // A Lua plugin sends `{}` for empty lists; the record must still decode.
        let wire = serde_json::json!({
            "meta": {
                "title": "Dune",
                "contributors": {},
                "isbns": {},
                "other_identifiers": {},
                "subjects": {},
            },
            "files": [{ "path": "Dune/dune.epub", "format": "epub" }],
            "cover_path": "Dune/cover.jpg",
        });
        let record = BookRecord::from_wire_json(wire).expect("record decodes");
        assert_eq!(record.meta.title, "Dune");
        assert!(record.meta.contributors.is_empty());
        assert_eq!(
            record.files,
            vec![BookFile {
                path: "Dune/dune.epub".into(),
                format: "epub".into(),
            }]
        );
        assert_eq!(record.cover_path.as_deref(), Some("Dune/cover.jpg"));
    }

    #[test]
    fn book_record_defaults_missing_files_and_cover() {
        let wire = serde_json::json!({ "meta": { "title": "T", "contributors": {}, "isbns": {}, "other_identifiers": {}, "subjects": {} } });
        let record = BookRecord::from_wire_json(wire).expect("record decodes");
        assert!(record.files.is_empty());
        assert!(record.cover_path.is_none());
    }
}
