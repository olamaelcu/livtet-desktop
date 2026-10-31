//! Reading what an edition currently holds, and writing an enrichment back.
//!
//! `read_snapshot` assembles the `EditionFields` the merge classifies against and
//! the `BookIdentity` an enricher keys on, in one pass, because both come from the
//! same edition row and its joins. `write_enrichment` writes an `AppliedFields`
//! set in one transaction and stores the cover afterwards, so a failed cover
//! write leaves the metadata applied rather than rolling it back (ADR-0046).
//!
//! This is the first code in the app that modifies edition metadata. Two
//! invariants hold here:
//!
//! - **Only what the source carried is written.** A field `AppliedFields` leaves
//!   `None` (or an empty list) is not touched, so applying an enrichment can
//!   never blank a column the source was silent about.
//! - **No `digital_inventory` row is ever created.** That table is the only
//!   source of the `has_file` filter and the card badges (ADR-0021), and a cover
//!   is not a file. An edition without a row simply gets no cover.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use camino::Utf8Path;
use livtet_core::data::{
    SharedState,
    entities::{
        authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers,
        edition_subjects, editions, identifiers, languages, publishers, subjects, works,
    },
    orm::{
        ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
        TransactionTrait,
    },
};
use livtet_importer::{BookIdentity, Identifier, ImporterPublicationDate};
use livtet_types::{DbId, now_primitive};

use super::catalog_write::{
    resolve_language, unique_preserving_order, upsert_author, upsert_identifier, upsert_publisher,
    upsert_subject, write_cover_file,
};
use super::enrich_merge::{AppliedFields, EditionFields};
use super::import::ImportError;

/// The role an enrichment's contributor binds with. `Enrichment` names people,
/// not roles, so everything it contributes is an author.
const ENRICHED_CONTRIBUTOR_ROLE: &str = "aut";

/// A failed snapshot read or enrichment write.
///
/// Shaped like [`ImportError`]: a stable machine `code` plus a message, so the
/// IPC layer can branch on the code without parsing prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrichStoreError {
    /// Stable machine code: `database` | `parse` | `io`.
    code: String,
    message: String,
}

impl EnrichStoreError {
    pub(crate) fn new(code: &str, message: impl std::fmt::Display) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }

    /// A storage failure, with the underlying error rendered into the message.
    pub(crate) fn database<E: std::fmt::Display>(err: E) -> Self {
        Self::new("database", err)
    }

    /// Stable machine code for this failure.
    pub fn code(&self) -> &str {
        &self.code
    }

    /// Human-readable detail.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<ImportError> for EnrichStoreError {
    fn from(error: ImportError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

impl std::fmt::Display for EnrichStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Enrich store error {}: {}",
            self.code, self.message
        ))
    }
}

impl std::error::Error for EnrichStoreError {}

/// What an edition holds right now, in the two shapes the enrichment pipeline
/// needs.
#[derive(Debug, Clone, PartialEq)]
pub struct EditionSnapshot {
    /// Current values the merge classifies against.
    pub fields: EditionFields,
    /// What an enricher keys on: identifiers, title, authors.
    pub identity: BookIdentity,
    /// Whether a `digital_inventory` row exists to hang a cover path on.
    pub cover_storable: bool,
    /// That row's id, for the `covers/{inventory_id}/cover.{ext}` path.
    pub inventory_id: Option<String>,
}

/// What writing an enrichment did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    /// Scalar columns written: `title`, `title_sort`, `publisher`,
    /// `description`, `language`, `published`. List membership (contributors,
    /// subjects) is linked in place and not counted here.
    pub fields_written: usize,
    /// Identifier links that did not exist before.
    pub identifiers_written: usize,
    /// Absolute path of the stored cover, when one was written and recorded.
    pub cover_path: Option<String>,
}

/// Read one edition's current metadata and identity.
///
/// `Ok(None)` when no edition with this id exists, matching
/// `get_edition_detail`: a missing row is a not-found, not an error.
pub async fn read_snapshot(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<EditionSnapshot>, EnrichStoreError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id)
        .one(db)
        .await
        .map_err(EnrichStoreError::database)?
    else {
        return Ok(None);
    };

    let work = works::Entity::find_by_id(edition.work_id)
        .one(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let contributors = contributor_names(db, edition_id).await?;
    let publisher = publisher_names(db, edition_id).await?.into_iter().next();
    let language = match edition.language_id {
        Some(id) => languages::Entity::find_by_id(id)
            .one(db)
            .await
            .map_err(EnrichStoreError::database)?
            .map(|language| language.code),
        None => None,
    };
    let subjects = subject_names(db, edition_id).await?;
    let identifiers = identifier_refs(db, edition_id).await?;
    let inventory = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let (cover_storable, inventory_id, has_cover) = match inventory {
        Some(row) => (true, Some(row.id.to_string()), row.cover_path.is_some()),
        None => (false, None, false),
    };

    // `editions.published_date` is a date, so it always carries a month and day;
    // a year-only source still compares as differing from it.
    let published = edition.published_date.map(|date| ImporterPublicationDate {
        year: date.year(),
        month: Some(date.month() as u8),
        day: Some(date.day()),
    });

    let title = edition.title.clone();
    let identity = BookIdentity {
        identifiers,
        // An edition without a title of its own is still the work's book, and a
        // title is the one thing every enricher can search on.
        title: title
            .clone()
            .or_else(|| work.as_ref().map(|work| work.title.clone())),
        authors: contributors.clone(),
    };

    Ok(Some(EditionSnapshot {
        fields: EditionFields {
            title,
            title_sort: work.and_then(|work| work.sort_title),
            contributors,
            publisher,
            language,
            published,
            description: edition.description.clone(),
            subjects,
            has_cover,
        },
        identity,
        cover_storable,
        inventory_id,
    }))
}

/// Write an `AppliedFields` set to one edition.
///
/// One transaction re-links contributors, the publisher, subjects and
/// identifiers and updates the scalar columns the source carried. The cover is
/// written after the commit, because a cover is re-derivable and a catalog row is
/// not: a failed cover write logs a warning and reports `cover_path: None`
/// instead of undoing the metadata (ADR-0046).
pub async fn write_enrichment(
    db: &SharedState,
    covers_dir: &Utf8Path,
    edition_id: DbId,
    applied: &AppliedFields,
) -> Result<ApplyOutcome, EnrichStoreError> {
    // Decode before opening the transaction: invalid base64 is a cover problem,
    // not a reason to hold a write lock or to lose the metadata.
    let cover = match &applied.cover {
        Some(cover) => match STANDARD.decode(&cover.data_base64) {
            Ok(bytes) => Some((cover.mime.as_str(), bytes)),
            Err(error) => {
                tracing::warn!(%error, "enrichment cover is not valid base64; skipping the cover");
                None
            }
        },
        None => None,
    };

    let conn = db.db_conn();
    let txn = conn.begin().await.map_err(EnrichStoreError::database)?;

    let edition = editions::Entity::find_by_id(edition_id)
        .one(&txn)
        .await
        .map_err(EnrichStoreError::database)?
        .ok_or_else(|| {
            EnrichStoreError::new("database", format!("edition {edition_id} no longer exists"))
        })?;

    let mut fields_written = 0usize;
    let mut identifiers_written = 0usize;

    // Scalars on the edition row. Every branch below runs only when the source
    // carried the field, so an absent value leaves the column as it was.
    let mut edition_model: editions::ActiveModel = edition.clone().into();
    if let Some(title) = &applied.title {
        edition_model.title = Set(Some(title.clone()));
        fields_written += 1;
    }
    if let Some(description) = &applied.description {
        edition_model.description = Set(Some(description.clone()));
        fields_written += 1;
    }
    if let Some(published) = &applied.published {
        edition_model.published_date = Set(publication_date(published));
        fields_written += 1;
    }
    if let Some(code) = applied.language.as_deref() {
        match resolve_language(&txn, Some(code)).await? {
            Some(language_id) => {
                edition_model.language_id = Set(Some(language_id));
                fields_written += 1;
            }
            None => tracing::warn!(
                code,
                "unrecognized language code; leaving the edition's language unchanged"
            ),
        }
    }
    if fields_written > 0 {
        edition_model.updated_at = Set(Some(now_primitive()));
        edition_model
            .update(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
    }

    // `title_sort` lives on the work, which is where the snapshot reads it from.
    if let Some(title_sort) = &applied.title_sort {
        let work = works::Entity::find_by_id(edition.work_id)
            .one(&txn)
            .await
            .map_err(EnrichStoreError::database)?
            .ok_or_else(|| {
                EnrichStoreError::new(
                    "database",
                    format!(
                        "work {} for edition {edition_id} is missing",
                        edition.work_id
                    ),
                )
            })?;
        let mut work_model: works::ActiveModel = work.into();
        work_model.sort_title = Set(Some(title_sort.clone()));
        work_model.updated_at = Set(Some(now_primitive()));
        work_model
            .update(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
        fields_written += 1;
    }

    if let Some(publisher) = &applied.publisher {
        let publisher_id = upsert_publisher(&txn, publisher).await?;
        let linked = edition_publishers::Entity::find()
            .filter(edition_publishers::Column::EditionId.eq(edition_id))
            .filter(edition_publishers::Column::PublisherId.eq(publisher_id))
            .one(&txn)
            .await
            .map_err(EnrichStoreError::database)?
            .is_some();
        if !linked {
            edition_publishers::ActiveModel {
                edition_id: Set(edition_id),
                publisher_id: Set(publisher_id),
            }
            .insert(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
        }
        fields_written += 1;
    }

    // Lists are the merge's final membership, so linking every member is
    // idempotent and never removes curation the source cannot see.
    for name in unique_preserving_order(&applied.contributors) {
        let author_id = upsert_author(&txn, &name).await?;
        // Match any role: a person already bound as `ill` must not also gain an
        // `aut` row just because the enrichment knows them as an author.
        let linked = edition_authors::Entity::find()
            .filter(edition_authors::Column::EditionId.eq(edition_id))
            .filter(edition_authors::Column::AuthorId.eq(author_id))
            .one(&txn)
            .await
            .map_err(EnrichStoreError::database)?
            .is_some();
        if !linked {
            edition_authors::ActiveModel {
                edition_id: Set(edition_id),
                author_id: Set(author_id),
                role: Set(ENRICHED_CONTRIBUTOR_ROLE.to_string()),
            }
            .insert(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
        }
    }

    for name in unique_preserving_order(&applied.subjects) {
        let subject_id = upsert_subject(&txn, &name).await?;
        let linked = edition_subjects::Entity::find()
            .filter(edition_subjects::Column::EditionId.eq(edition_id))
            .filter(edition_subjects::Column::SubjectId.eq(subject_id))
            .one(&txn)
            .await
            .map_err(EnrichStoreError::database)?
            .is_some();
        if !linked {
            edition_subjects::ActiveModel {
                edition_id: Set(edition_id),
                subject_id: Set(subject_id),
            }
            .insert(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
        }
    }

    for identifier in &applied.identifiers {
        if identifier.kind.trim().is_empty() || identifier.value.trim().is_empty() {
            tracing::debug!(
                kind = %identifier.kind,
                "enrichment identifier has a blank kind or value; skipping"
            );
            continue;
        }
        let identifier_id = upsert_identifier(&txn, &identifier.value, &identifier.kind).await?;
        let linked = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(edition_id))
            .filter(edition_identifiers::Column::IdentifierId.eq(identifier_id))
            .one(&txn)
            .await
            .map_err(EnrichStoreError::database)?
            .is_some();
        if !linked {
            edition_identifiers::ActiveModel {
                edition_id: Set(edition_id),
                identifier_id: Set(identifier_id),
            }
            .insert(&txn)
            .await
            .map_err(EnrichStoreError::database)?;
            identifiers_written += 1;
        }
    }

    let inventory_id = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(&txn)
        .await
        .map_err(EnrichStoreError::database)?
        .map(|row| row.id);

    txn.commit().await.map_err(EnrichStoreError::database)?;

    // Post-commit: the metadata is authoritative already. A cover that cannot be
    // written or recorded leaves it applied and reports no path.
    let cover_path = match (cover, inventory_id) {
        (Some((mime, bytes)), Some(inventory_id)) => {
            let path = covers_dir
                .join(inventory_id.to_string())
                .join(format!("cover.{}", cover_extension(mime)))
                .to_string();
            match write_cover_file(&path, &bytes).await {
                Ok(()) => record_cover_path(&conn, inventory_id, &path).await,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        %path,
                        "cover write failed; edition metadata stays applied"
                    );
                    None
                }
            }
        }
        (Some(_), None) => {
            tracing::debug!(
                edition_id = %edition_id,
                "edition has no digital inventory row; skipping the cover"
            );
            None
        }
        (None, _) => None,
    };

    Ok(ApplyOutcome {
        fields_written,
        identifiers_written,
        cover_path,
    })
}

/// Store the cover path on the inventory row, reporting it only once it is
/// recorded. Every failure here is a warning: the transaction already committed.
async fn record_cover_path(
    conn: &DatabaseConnection,
    inventory_id: DbId,
    path: &str,
) -> Option<String> {
    let row = match digital_inventory::Entity::find_by_id(inventory_id)
        .one(conn)
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => {
            tracing::warn!(
                inventory_id = %inventory_id,
                "inventory row vanished after commit; cover path not recorded"
            );
            return None;
        }
        Err(error) => {
            tracing::warn!(%error, %path, "cover path lookup failed; cover not recorded");
            return None;
        }
    };
    let mut model: digital_inventory::ActiveModel = row.into();
    model.cover_path = Set(Some(path.to_string()));
    model.updated_at = Set(Some(now_primitive()));
    match model.update(conn).await {
        Ok(_) => Some(path.to_string()),
        Err(error) => {
            tracing::warn!(%error, %path, "cover path update failed; cover not recorded");
            None
        }
    }
}

/// Names linked to an edition, in link order.
async fn contributor_names(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<String>, EnrichStoreError> {
    let links = edition_authors::Entity::find()
        .filter(edition_authors::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.author_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let names: std::collections::HashMap<DbId, String> = authors::Entity::find()
        .filter(authors::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?
        .into_iter()
        .map(|author| (author.id, author.name))
        .collect();

    Ok(links
        .into_iter()
        .filter_map(|link| names.get(&link.author_id).cloned())
        .collect())
}

async fn publisher_names(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<String>, EnrichStoreError> {
    let links = edition_publishers::Entity::find()
        .filter(edition_publishers::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.publisher_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(publishers::Entity::find()
        .filter(publishers::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?
        .into_iter()
        .map(|publisher| publisher.name)
        .collect())
}

async fn subject_names(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<String>, EnrichStoreError> {
    let links = edition_subjects::Entity::find()
        .filter(edition_subjects::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.subject_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(subjects::Entity::find()
        .filter(subjects::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?
        .into_iter()
        .map(|subject| subject.name)
        .collect())
}

async fn identifier_refs(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Vec<Identifier>, EnrichStoreError> {
    let links = edition_identifiers::Entity::find()
        .filter(edition_identifiers::Column::EditionId.eq(edition_id))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?;

    let ids: Vec<DbId> = links.iter().map(|link| link.identifier_id).collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }

    Ok(identifiers::Entity::find()
        .filter(identifiers::Column::Id.is_in(ids))
        .all(db)
        .await
        .map_err(EnrichStoreError::database)?
        .into_iter()
        .map(|identifier| Identifier {
            kind: identifier.kind,
            value: identifier.value,
        })
        .collect())
}

/// The stored calendar date for a publication date. A year-only or year-month
/// value lands on the first day of the missing component, the same widening
/// `persist_import` does; a date SQLite cannot hold becomes `NULL` rather than
/// failing the write.
fn publication_date(published: &ImporterPublicationDate) -> Option<time::Date> {
    time::Date::from_calendar_date(
        published.year,
        time::Month::try_from(published.month.unwrap_or(1)).unwrap_or(time::Month::January),
        published.day.unwrap_or(1),
    )
    .ok()
}

/// File extension for a cover MIME type.
///
/// The same table `import::cover_extension` uses (private there): both callers
/// write ADR-0008's `covers/{inventory_id}/cover.{ext}` path, so they must agree.
fn cover_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "img",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use livtet_core::data::orm::Set;
    use livtet_core::data::{Kind, TestDb};
    use livtet_importer::ImporterCover;
    use livtet_types::now_primitive;

    use super::*;

    struct Seed {
        edition_id: DbId,
        inventory_id: Option<DbId>,
    }

    /// One edition with a work, an author, a publisher, a subject, an ISBN, and
    /// (optionally) a digital inventory row carrying a cover.
    async fn seed_edition(db: &DatabaseConnection, with_inventory: bool) -> Seed {
        let now = now_primitive();
        let language_id = DbId::new();
        languages::ActiveModel {
            id: Set(language_id),
            name: Set("English".to_string()),
            code: Set("eng".to_string()),
            flag_emoji: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("language");

        let work_id = DbId::new();
        works::ActiveModel {
            id: Set(work_id),
            title: Set("The Book".to_string()),
            description: Set(Some("A work".to_string())),
            sort_title: Set(Some("Book, The".to_string())),
            series_type: Set(None),
            language_id: Set(Some(language_id)),
            preferred_edition_id: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("work");

        let edition_id = DbId::new();
        editions::ActiveModel {
            id: Set(edition_id),
            work_id: Set(work_id),
            group_id: Set(None),
            title: Set(Some("The Book".to_string())),
            published_date: Set(Some(
                time::Date::from_calendar_date(2001, time::Month::September, 11).expect("date"),
            )),
            format_id: Set(None),
            language_id: Set(Some(language_id)),
            notes: Set(None),
            description: Set(Some("An edition".to_string())),
            format_metadata: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("edition");

        let author_id = DbId::new();
        authors::ActiveModel {
            id: Set(author_id),
            name: Set("Ada Lovelace".to_string()),
        }
        .insert(db)
        .await
        .expect("author");
        edition_authors::ActiveModel {
            edition_id: Set(edition_id),
            author_id: Set(author_id),
            role: Set("aut".to_string()),
        }
        .insert(db)
        .await
        .expect("edition author");

        let publisher_id = DbId::new();
        publishers::ActiveModel {
            id: Set(publisher_id),
            name: Set("Acme Press".to_string()),
            website: Set(None),
            logo_url: Set(None),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("publisher");
        edition_publishers::ActiveModel {
            edition_id: Set(edition_id),
            publisher_id: Set(publisher_id),
        }
        .insert(db)
        .await
        .expect("edition publisher");

        let subject_id = DbId::new();
        subjects::ActiveModel {
            id: Set(subject_id),
            name: Set("Lighthouses".to_string()),
            created_at: Set(now),
            updated_at: Set(None),
        }
        .insert(db)
        .await
        .expect("subject");
        edition_subjects::ActiveModel {
            edition_id: Set(edition_id),
            subject_id: Set(subject_id),
        }
        .insert(db)
        .await
        .expect("edition subject");

        let identifier_id = DbId::new();
        identifiers::ActiveModel {
            id: Set(identifier_id),
            value: Set("urn:isbn:9780306406157".to_string()),
            kind: Set("isbn".to_string()),
        }
        .insert(db)
        .await
        .expect("identifier");
        edition_identifiers::ActiveModel {
            edition_id: Set(edition_id),
            identifier_id: Set(identifier_id),
        }
        .insert(db)
        .await
        .expect("edition identifier");

        let inventory_id = if with_inventory {
            let inventory_id = DbId::new();
            digital_inventory::ActiveModel {
                id: Set(inventory_id),
                edition_id: Set(edition_id),
                file_path: Set(Some("/books/the-book.epub".to_string())),
                cover_path: Set(Some("/covers/x/cover.jpg".to_string())),
                blurhash: Set(None),
                dominant_color: Set(None),
                file_hash: Set(Some("deadbeef".to_string())),
                file_size_bytes: Set(Some(1024)),
                file_format: Set(Some("epub".to_string())),
                notes: Set(None),
                added_at: Set(now),
                updated_at: Set(None),
            }
            .insert(db)
            .await
            .expect("digital inventory");
            Some(inventory_id)
        } else {
            None
        };

        Seed {
            edition_id,
            inventory_id,
        }
    }

    fn temporary_covers_dir() -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = camino::Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).expect("utf8");
        (dir, path.join("covers"))
    }

    #[tokio::test]
    async fn reads_current_fields_and_identity() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();
        let seed = seed_edition(&db, true).await;

        let snapshot = read_snapshot(&db, seed.edition_id)
            .await
            .expect("read ok")
            .expect("edition present");

        assert_eq!(snapshot.fields.title.as_deref(), Some("The Book"));
        assert_eq!(snapshot.fields.title_sort.as_deref(), Some("Book, The"));
        assert_eq!(
            snapshot.fields.contributors,
            vec!["Ada Lovelace".to_string()]
        );
        assert_eq!(snapshot.fields.publisher.as_deref(), Some("Acme Press"));
        assert_eq!(snapshot.fields.language.as_deref(), Some("eng"));
        assert_eq!(
            snapshot.fields.published,
            Some(ImporterPublicationDate {
                year: 2001,
                month: Some(9),
                day: Some(11),
            })
        );
        assert_eq!(snapshot.fields.description.as_deref(), Some("An edition"));
        assert_eq!(snapshot.fields.subjects, vec!["Lighthouses".to_string()]);
        assert!(snapshot.fields.has_cover);

        assert_eq!(snapshot.identity.title.as_deref(), Some("The Book"));
        assert_eq!(snapshot.identity.authors, snapshot.fields.contributors);
        assert_eq!(
            snapshot.identity.identifiers,
            vec![Identifier {
                kind: "isbn".to_string(),
                value: "urn:isbn:9780306406157".to_string(),
            }]
        );

        assert!(snapshot.cover_storable);
        assert_eq!(
            snapshot.inventory_id,
            seed.inventory_id.map(|id| id.to_string())
        );
    }

    #[tokio::test]
    async fn missing_edition_reads_none() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let db = test_db.state().db_conn();

        let snapshot = read_snapshot(&db, DbId::new()).await.expect("read ok");
        assert!(snapshot.is_none());
    }

    #[tokio::test]
    async fn writes_only_the_fields_the_source_carried() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let db = state.db_conn();
        let seed = seed_edition(&db, true).await;
        let (_covers, covers_dir) = temporary_covers_dir();

        let applied = AppliedFields {
            title: Some("The Book, Revised".to_string()),
            title_sort: None,
            publisher: None,
            description: None,
            language: Some("fra".to_string()),
            published: None,
            cover: None,
            contributors: vec!["Ada Lovelace".to_string(), "Grace Hopper".to_string()],
            subjects: vec!["Lighthouses".to_string(), "Computing".to_string()],
            identifiers: Vec::new(),
        };

        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");

        assert_eq!(outcome.fields_written, 2, "title and language");
        assert_eq!(outcome.identifiers_written, 0);
        assert_eq!(outcome.cover_path, None);

        let edition = editions::Entity::find_by_id(seed.edition_id)
            .one(&db)
            .await
            .expect("query ok")
            .expect("edition");
        assert_eq!(edition.title.as_deref(), Some("The Book, Revised"));
        // Silent fields are untouched: the source never spoke about them.
        assert_eq!(edition.description.as_deref(), Some("An edition"));
        assert_eq!(
            edition.published_date,
            Some(time::Date::from_calendar_date(2001, time::Month::September, 11).expect("date"))
        );

        let language = languages::Entity::find_by_id(edition.language_id.expect("language id"))
            .one(&db)
            .await
            .expect("query ok")
            .expect("language");
        assert_eq!(language.code, "fr");
        assert_eq!(language.name, "French");

        let snapshot = read_snapshot(&db, seed.edition_id)
            .await
            .expect("read ok")
            .expect("edition present");
        assert_eq!(
            snapshot.fields.contributors,
            vec!["Ada Lovelace".to_string(), "Grace Hopper".to_string()]
        );
        assert_eq!(
            snapshot.fields.subjects,
            vec!["Lighthouses".to_string(), "Computing".to_string()]
        );
        assert_eq!(snapshot.fields.title_sort.as_deref(), Some("Book, The"));
    }

    #[tokio::test]
    async fn links_the_publisher_and_title_sort_it_carries() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let db = state.db_conn();
        let seed = seed_edition(&db, true).await;
        let (_covers, covers_dir) = temporary_covers_dir();

        let applied = AppliedFields {
            title: None,
            title_sort: Some("Book, The (revised)".to_string()),
            publisher: Some("Second Press".to_string()),
            description: None,
            language: None,
            published: None,
            cover: None,
            contributors: Vec::new(),
            subjects: Vec::new(),
            identifiers: Vec::new(),
        };

        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");
        assert_eq!(outcome.fields_written, 2, "title_sort and publisher");

        let snapshot = read_snapshot(&db, seed.edition_id)
            .await
            .expect("read ok")
            .expect("edition present");
        assert_eq!(
            snapshot.fields.title_sort.as_deref(),
            Some("Book, The (revised)")
        );
        // The existing publisher is kept (the merge never removes membership), so
        // the second one appends rather than replaces.
        let mut names = publisher_names(&db, seed.edition_id)
            .await
            .expect("names ok");
        names.sort();
        assert_eq!(
            names,
            vec!["Acme Press".to_string(), "Second Press".to_string()]
        );
    }

    #[tokio::test]
    async fn existing_identifiers_are_not_duplicated() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let db = state.db_conn();
        let seed = seed_edition(&db, true).await;
        let (_covers, covers_dir) = temporary_covers_dir();

        let applied = AppliedFields {
            title: None,
            title_sort: None,
            publisher: None,
            description: None,
            language: None,
            published: None,
            cover: None,
            contributors: Vec::new(),
            subjects: Vec::new(),
            identifiers: vec![
                Identifier {
                    kind: "isbn".to_string(),
                    value: "urn:isbn:9780306406157".to_string(),
                },
                Identifier {
                    kind: "olid".to_string(),
                    value: "OL12345W".to_string(),
                },
            ],
        };

        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");
        assert_eq!(outcome.identifiers_written, 1, "only the OLID is new");

        // Re-applying the same set adds nothing.
        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");
        assert_eq!(outcome.identifiers_written, 0);

        let links = edition_identifiers::Entity::find()
            .filter(edition_identifiers::Column::EditionId.eq(seed.edition_id))
            .all(&db)
            .await
            .expect("query ok");
        assert_eq!(links.len(), 2, "the ISBN is linked once");

        let isbn_rows = identifiers::Entity::find()
            .filter(identifiers::Column::Value.eq("urn:isbn:9780306406157"))
            .all(&db)
            .await
            .expect("query ok");
        assert_eq!(isbn_rows.len(), 1, "one identifiers row, not two");
    }

    #[tokio::test]
    async fn edition_without_inventory_gets_no_cover() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let db = state.db_conn();
        let seed = seed_edition(&db, false).await;
        let (_covers, covers_dir) = temporary_covers_dir();

        let snapshot = read_snapshot(&db, seed.edition_id)
            .await
            .expect("read ok")
            .expect("edition present");
        assert!(!snapshot.cover_storable);
        assert!(!snapshot.fields.has_cover);
        assert_eq!(snapshot.inventory_id, None);

        let applied = AppliedFields {
            title: Some("The Book".to_string()),
            cover: Some(ImporterCover {
                mime: "image/png".to_string(),
                data_base64: STANDARD.encode(b"not-really-a-png"),
            }),
            ..Default::default()
        };

        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");
        assert_eq!(outcome.cover_path, None);
        assert_eq!(outcome.fields_written, 1);

        let inventory = digital_inventory::Entity::find()
            .filter(digital_inventory::Column::EditionId.eq(seed.edition_id))
            .one(&db)
            .await
            .expect("query ok");
        assert!(inventory.is_none(), "enrichment must not invent a file row");
        assert!(!covers_dir.exists(), "no cover dir for an unstorable cover");
    }

    #[tokio::test]
    async fn stores_cover_after_commit_and_records_its_path() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let db = state.db_conn();
        let seed = seed_edition(&db, true).await;
        let (_covers, covers_dir) = temporary_covers_dir();

        let appended = b"fake-png-bytes";
        let applied = AppliedFields {
            title: Some("The Book, Revised".to_string()),
            cover: Some(ImporterCover {
                mime: "image/png".to_string(),
                data_base64: STANDARD.encode(appended),
            }),
            ..Default::default()
        };

        let outcome = write_enrichment(&state, &covers_dir, seed.edition_id, &applied)
            .await
            .expect("apply ok");

        let expected = covers_dir
            .join(seed.inventory_id.expect("inventory").to_string())
            .join("cover.png");
        assert_eq!(outcome.cover_path.as_deref(), Some(expected.as_str()));
        assert_eq!(fs_err::read(&expected).expect("cover written"), appended);

        let inventory = digital_inventory::Entity::find_by_id(seed.inventory_id.expect("id"))
            .one(&db)
            .await
            .expect("query ok")
            .expect("inventory row");
        assert_eq!(inventory.cover_path.as_deref(), Some(expected.as_str()));

        // The metadata landed in the same call, independent of the cover.
        let edition = editions::Entity::find_by_id(seed.edition_id)
            .one(&db)
            .await
            .expect("query ok")
            .expect("edition");
        assert_eq!(edition.title.as_deref(), Some("The Book, Revised"));
        assert_eq!(outcome.fields_written, 1);
    }

    #[tokio::test]
    async fn cover_extension_follows_the_mime_type() {
        assert_eq!(cover_extension("image/jpeg"), "jpg");
        assert_eq!(cover_extension("image/png"), "png");
        assert_eq!(cover_extension("image/webp"), "webp");
        assert_eq!(cover_extension("image/gif"), "gif");
        assert_eq!(cover_extension("application/octet-stream"), "img");
    }

    #[tokio::test]
    async fn missing_edition_fails_the_write() {
        let test_db = TestDb::new(&[Kind::Business]).await.expect("test db");
        let state = test_db.state();
        let (_covers, covers_dir) = temporary_covers_dir();

        let error = write_enrichment(&state, &covers_dir, DbId::new(), &AppliedFields::default())
            .await
            .expect_err("missing edition is an error");
        assert_eq!(error.code(), "database");
        assert!(error.message().contains("no longer exists"));
    }
}
