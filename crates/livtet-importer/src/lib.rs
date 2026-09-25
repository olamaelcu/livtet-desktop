//! Typed contract for Livtet file importers.
//!
//! The contract is intentionally small: native and Lua-backed importers agree on
//! which extensions they handle and on the bibliographic record they return.
//! Remote callers use [`ImporterMeta`] as the JSON boundary.

mod epub;
mod mobi;

pub use epub::EpubImporter;
pub use mobi::MobiImporter;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};
use stanchion::{
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

pub(crate) fn role_string(role: &livtet_epub::Role) -> String {
    match role {
        livtet_epub::Role::Author => "aut",
        livtet_epub::Role::Editor => "edt",
        livtet_epub::Role::Translator => "trl",
        livtet_epub::Role::Illustrator => "ill",
        livtet_epub::Role::Other(raw) => raw,
    }
    .to_string()
}
