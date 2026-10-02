//! Shared lossy-Lua-to-JSON normalization helpers for Livtet importers.
//!
//! These functions convert a raw JSON wire representation (as received from a Lua
//! plugin or importer) into a strongly-typed Rust `ImporterMeta` / `Enrichment`
//! record. They handle the fact that Lua tables arrive as `{}` for both empty lists
//! and empty objects – the host must distinguish them.

use serde_json::{Map, Value as Json};

/// Convert an empty JSON object to `null` so that the caller can tell
/// whether a field was absent or simply empty.
pub fn normalize_optional_object(value: &mut Json) {
    if matches!(value, Json::Object(entries) if entries.is_empty()) {
        *value = Json::Null;
    }
}

/// Recursively normalize optional scalar/object fields in a map.
pub fn normalize_optional_fields(record: &mut Map<String, Json>, fields: &[&str]) {
    for field in fields {
        if let Some(value) = record.get_mut(*field) {
            normalize_optional_object(value);
        }
    }
}

/// Normalize optional contributor fields (`role`, `file_as`) inside each entry
/// of a `contributors` array.
pub fn normalize_contributor_optionals(record: &mut Map<String, Json>) {
    let Some(Json::Array(contributors)) = record.get_mut("contributors") else {
        return;
    };
    for contributor in contributors {
        if let Some(contributor) = contributor.as_object_mut() {
            normalize_optional_fields(contributor, &["role", "file_as"]);
        }
    }
}

/// Normalize optional publication-date fields (`month`, `day`) inside the
/// `published` object.
pub fn normalize_publication_optionals(record: &mut Map<String, Json>) {
    let Some(published) = record.get_mut("published") else {
        return;
    };
    if let Some(published) = published.as_object_mut() {
        normalize_optional_fields(published, &["month", "day"]);
    }
}

/// Convert empty-object list fields to empty arrays, since Lua cannot
/// distinguish the two.
pub fn normalize_list_fields(record: &mut Map<String, Json>, fields: &[&str]) {
    for field in fields {
        if let Some(list) = record.get_mut(*field)
            && let Json::Object(entries) = list
            && entries.is_empty()
        {
            *list = Json::Array(Vec::new());
        }
    }
}

/// Parse a raw `ImporterMeta` JSON wire into a typed `ImporterMeta`.
/// Handles the lossy-Lua boundary: empty tables become `null`.
pub fn parse_importer_meta(
    mut value: Json,
) -> std::result::Result<super::ImporterMeta, serde_json::Error> {
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
        normalize_list_fields(
            record,
            &["contributors", "isbns", "other_identifiers", "subjects"],
        );
    }
    serde_json::from_value(value)
}

/// Parse a raw `Enrichment` JSON wire into a typed `Enrichment`.
/// Mirrors `ImporterMeta::from_wire_json` but adds the `found` vector of identifiers.
pub fn parse_enrichment(
    mut value: Json,
) -> std::result::Result<super::Enrichment, serde_json::Error> {
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
        normalize_list_fields(record, &["contributors", "subjects", "found"]);
    }
    serde_json::from_value(value)
}
