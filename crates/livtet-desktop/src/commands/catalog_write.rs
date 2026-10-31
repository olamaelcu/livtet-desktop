//! Catalog writes shared by more than one command.
//!
//! Import builds a record from a file; enrichment fills one from a metadata
//! source. Both need the same "ensure this contributor/publisher/subject exists,
//! then link it" steps, and duplicating them across `import.rs` (already past
//! 1400 lines) and the enrichment writer would let the two drift. Everything
//! here takes an open transaction so a caller composes several writes atomically.
//!
//! The callers are [`super::import`] (`import_file` / `import_files`) and the
//! enrichment writer in [`super::enrich`].

use livtet_core::data::{
    entities::{authors, identifiers, languages, publishers, subjects},
    orm::{ActiveModelTrait, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set},
};
use livtet_importer::ImporterContributor;
use livtet_types::{CommonLanguages, DbId, now_primitive};

use super::import::{ImportError, normalize_contributor_role};

/// Find-or-create a name-keyed entity, returning its id. `insert` builds the
/// model with an id; we keep that id in both code paths.
pub(crate) async fn upsert_author(
    txn: &DatabaseTransaction,
    name: &str,
) -> Result<DbId, ImportError> {
    if let Some(found) = authors::Entity::find()
        .filter(authors::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    authors::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

pub(crate) async fn upsert_identifier(
    txn: &DatabaseTransaction,
    value: &str,
    kind: &str,
) -> Result<DbId, ImportError> {
    if let Some(found) = identifiers::Entity::find()
        .filter(identifiers::Column::Value.eq(value))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    identifiers::ActiveModel {
        id: Set(id),
        value: Set(value.to_string()),
        kind: Set(kind.to_string()),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

pub(crate) async fn upsert_publisher(
    txn: &DatabaseTransaction,
    name: &str,
) -> Result<DbId, ImportError> {
    if let Some(found) = publishers::Entity::find()
        .filter(publishers::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    publishers::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        website: Set(None),
        logo_url: Set(None),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

pub(crate) async fn upsert_subject(
    txn: &DatabaseTransaction,
    name: &str,
) -> Result<DbId, ImportError> {
    if let Some(found) = subjects::Entity::find()
        .filter(subjects::Column::Name.eq(name))
        .one(txn)
        .await?
    {
        return Ok(found.id);
    }
    let id = DbId::new();
    subjects::ActiveModel {
        id: Set(id),
        name: Set(name.to_string()),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(id)
}

/// Resolve the `languages` row for the normalized code, creating it when the
/// language is not in the seeded set.
pub(crate) async fn resolve_language(
    txn: &DatabaseTransaction,
    language: Option<&str>,
) -> Result<Option<DbId>, ImportError> {
    let Some(raw) = language else { return Ok(None) };
    let Some(info) = CommonLanguages::normalize_language_code(raw) else {
        return Ok(None);
    };
    if let Some(found) = languages::Entity::find()
        .filter(languages::Column::Code.eq(&info.code))
        .one(txn)
        .await?
    {
        return Ok(Some(found.id));
    }
    let id = DbId::new();
    languages::ActiveModel {
        id: Set(id),
        name: Set(info.english_name),
        code: Set(info.code),
        flag_emoji: Set(info.flag_emoji),
        created_at: Set(now_primitive()),
        updated_at: Set(None),
    }
    .insert(txn)
    .await?;
    Ok(Some(id))
}

/// Write cover bytes to `path`, creating its parent directory.
///
/// Callers record the path in the database, so a failure here means "null the
/// path", not "fail the write": a cover is re-derivable, a catalog row is not.
pub(crate) async fn write_cover_file(path: &str, bytes: &[u8]) -> Result<(), String> {
    let path = camino::Utf8Path::new(path);
    if let Some(parent) = path.parent() {
        fs_err::tokio::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
    }
    fs_err::tokio::write(path, bytes)
        .await
        .map_err(|e| e.to_string())
}

/// Collapse a list of strings to its unique non-empty entries, in first-seen
/// order. Importers already dedupe their output, but this fence guarantees a
/// repeated subject or ISBN can never violate a composite primary key on
/// `edition_subjects` / `edition_identifiers` regardless of what it hands us.
pub(crate) fn unique_preserving_order(values: &[String]) -> Vec<String> {
    let mut unique: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !unique.iter().any(|seen| seen == value) {
            unique.push(value.to_string());
        }
    }
    unique
}

/// Collapse contributor records to the `(name, normalized_role)` pairs that
/// become `edition_authors` rows, in input order and deduplicated. The importer
/// already dedupes its output, but this fence guarantees a repeated binding can
/// never violate the composite primary key regardless of what it hands us.
pub(crate) fn contributor_bindings(contributors: &[ImporterContributor]) -> Vec<(String, String)> {
    let mut bindings: Vec<(String, String)> = Vec::with_capacity(contributors.len());
    for contributor in contributors {
        let (role, _) = normalize_contributor_role(contributor.role.as_deref());
        let binding = (contributor.name.clone(), role);
        if !bindings.contains(&binding) {
            bindings.push(binding);
        }
    }
    bindings
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use livtet_importer::ImporterContributor;

    use super::contributor_bindings;

    /// The `(name, role)` bindings produced for the given `(name, role)` input.
    fn bindings(pairs: &[(&str, Option<&str>)]) -> Vec<(String, String)> {
        let contributors: Vec<ImporterContributor> = pairs
            .iter()
            .map(|(name, role)| ImporterContributor {
                name: (*name).to_string(),
                role: role.map(str::to_string),
                file_as: None,
            })
            .collect();
        contributor_bindings(&contributors)
    }

    #[test]
    fn contributor_bindings_dedupes_same_name_and_role() {
        assert_eq!(
            bindings(&[
                ("Susana M. Morris", Some("aut")),
                ("Susana M. Morris", Some("aut")),
            ]),
            vec![("Susana M. Morris".to_string(), "aut".to_string())]
        );
    }

    #[test]
    fn contributor_bindings_keeps_distinct_roles() {
        assert_eq!(
            bindings(&[
                ("Susana M. Morris", Some("aut")),
                ("Susana M. Morris", Some("ill")),
            ]),
            vec![
                ("Susana M. Morris".to_string(), "aut".to_string()),
                ("Susana M. Morris".to_string(), "ill".to_string()),
            ]
        );
    }

    #[test]
    fn contributor_bindings_blank_role_defaults_to_aut() {
        assert_eq!(
            bindings(&[("Susana M. Morris", None), ("Susana M. Morris", Some("  ")),]),
            vec![("Susana M. Morris".to_string(), "aut".to_string())]
        );
    }

    #[test]
    fn contributor_bindings_preserves_order() {
        assert_eq!(
            bindings(&[
                ("First", Some("aut")),
                ("Second", Some("edt")),
                ("Third", Some("ill")),
            ]),
            vec![
                ("First".to_string(), "aut".to_string()),
                ("Second".to_string(), "edt".to_string()),
                ("Third".to_string(), "ill".to_string()),
            ]
        );
    }
}
