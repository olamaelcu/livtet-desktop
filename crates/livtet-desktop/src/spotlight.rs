//! macOS Spotlight indexing for the library (opt-in).
//!
//! The setting and the Sync orchestration live here so the commands compile on
//! every platform; the only platform-specific pieces are in [`macos`]. One
//! document per edition is written to the default CoreSpotlight index under a
//! stable domain, which lets "Rebuild" delete stale rows in one call.
//!
//! Selecting a Spotlight result is handled by the app-delegate swizzle in
//! [`macos::install_activation`]: tao only forwards browsing activities, so
//! without it the OS would launch the app but drop the edition identifier.

use std::collections::HashMap;
use std::sync::Arc;

use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_subjects,
    edition_tags, editions, identifiers, subjects, tags, works,
};
// The `FromQueryResult` derive expands to `sea_orm::…` paths, so the ORM crate
// must be reachable under that name.
use livtet_core::data::orm as sea_orm;
use livtet_core::data::orm::{DatabaseConnection, EntityTrait, FromQueryResult, QuerySelect};
use livtet_types::DbId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Wry;
use tokio::sync::Mutex;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::install_activation;

/// Domain that owns every Livtet item; deleting it removes stale editions.
pub(crate) const DOMAIN: &str = "net.olamaelcu.livtet.library";

/// Event that asks the web layer to open one edition's detail drawer.
pub(crate) const OPEN_EDITION_EVENT: &str = "spotlight://open-edition";

const SETTINGS_KEY: &str = "spotlight";

/// How Spotlight indexing is configured, persisted under [`SETTINGS_KEY`].
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct SpotlightSettings {
    /// Whether the library is indexed at all.
    pub enabled: bool,
    /// RFC 3339 timestamp of the last successful index write.
    pub indexed_at: Option<String>,
    /// How many editions the last write covered.
    pub item_count: u32,
}

/// What Settings shows: the persisted preference plus platform availability.
#[derive(Debug, Clone, Serialize, Type)]
pub struct SpotlightStatus {
    /// False on platforms without CoreSpotlight, and on macOS builds whose
    /// unsigned state prevents indexing.
    pub available: bool,
    pub enabled: bool,
    pub indexed_at: Option<String>,
    pub item_count: u32,
}

/// A Spotlight failure, shaped like the other command errors (`{ code, message }`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct SpotlightError {
    /// Stable reason: `unsupported`, `storage`, `index`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl SpotlightError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SpotlightError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

/// Where the setting lives. A trait so the `AppState` fixture can use memory.
pub(crate) trait SpotlightSettingsStore: Send + Sync {
    fn read(&self) -> Option<SpotlightSettings>;
    fn write(&self, settings: &SpotlightSettings) -> Result<(), String>;
}

impl SpotlightSettingsStore for tauri_plugin_store::Store<Wry> {
    fn read(&self) -> Option<SpotlightSettings> {
        self.get(SETTINGS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    fn write(&self, settings: &SpotlightSettings) -> Result<(), String> {
        let value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
        self.set(SETTINGS_KEY, value);
        self.save().map_err(|error| error.to_string())
    }
}

// The library can contain rows whose nullable `format_id`/`language_id` are
// zero-length blobs (an import wrote them that way). Decoding the whole entity
// fails on those, so every Spotlight query projects only the columns it reads.
#[derive(FromQueryResult)]
struct EditionRow {
    id: DbId,
    work_id: DbId,
    title: Option<String>,
    description: Option<String>,
}

#[derive(FromQueryResult)]
struct WorkRow {
    id: DbId,
    title: String,
    description: Option<String>,
}

#[derive(FromQueryResult)]
struct NameRow {
    id: DbId,
    name: String,
}

#[derive(FromQueryResult)]
struct IdentifierRow {
    id: DbId,
    value: String,
}

#[derive(FromQueryResult)]
struct EditionAuthorRow {
    edition_id: DbId,
    author_id: DbId,
}

#[derive(FromQueryResult)]
struct EditionTagRow {
    edition_id: DbId,
    tag_id: DbId,
}

#[derive(FromQueryResult)]
struct EditionSubjectRow {
    edition_id: DbId,
    subject_id: DbId,
}

#[derive(FromQueryResult)]
struct EditionIdentifierRow {
    edition_id: DbId,
    identifier_id: DbId,
}

#[derive(FromQueryResult)]
struct CoverRow {
    edition_id: DbId,
    cover_path: Option<String>,
}

/// One edition as Spotlight sees it.
pub(crate) struct SpotlightRecord {
    pub edition_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub description: Option<String>,
    /// Tags, subjects, and identifiers, all free-text match terms.
    pub keywords: Vec<String>,
    pub cover_path: Option<String>,
}

/// Persisted preference plus a lock that serializes index writes.
pub struct SpotlightRuntime {
    store: Arc<dyn SpotlightSettingsStore>,
    index_lock: Mutex<()>,
    /// True while a debounced reindex is queued or running.
    scheduled: std::sync::atomic::AtomicBool,
}

impl SpotlightRuntime {
    pub(crate) fn new(store: Arc<dyn SpotlightSettingsStore>) -> Self {
        Self {
            store,
            index_lock: Mutex::new(()),
            scheduled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Ask for a refresh after a mutation without waiting for it.
    ///
    /// Calls coalesce: the first one arms a short timer, and everything that
    /// happens while it runs is covered by the same rebuild.
    pub fn schedule_reindex(self: &Arc<Self>, db: livtet_core::data::SharedState) {
        use std::sync::atomic::Ordering;

        if !self.enabled() {
            return;
        }
        if self.scheduled.swap(true, Ordering::AcqRel) {
            return;
        }

        let runtime = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            runtime.scheduled.store(false, Ordering::Release);
            if let Err(error) = runtime.reindex(&db.db_conn()).await {
                tracing::warn!(%error, "Spotlight refresh after a mutation failed");
            }
        });
    }

    pub fn settings(&self) -> SpotlightSettings {
        self.store.read().unwrap_or_default()
    }

    pub fn enabled(&self) -> bool {
        self.settings().enabled
    }

    pub fn status(&self) -> SpotlightStatus {
        let settings = self.settings();
        SpotlightStatus {
            available: available(),
            enabled: settings.enabled && available(),
            indexed_at: settings.indexed_at,
            item_count: settings.item_count,
        }
    }

    /// Turn indexing on (writing the whole library) or off (deleting the
    /// domain). Persists the flag only after the index write succeeds, so a
    /// failed first index never leaves the UI claiming it is on.
    pub async fn set_enabled(
        &self,
        db: &DatabaseConnection,
        enabled: bool,
    ) -> Result<SpotlightStatus, SpotlightError> {
        let _guard = self.index_lock.lock().await;

        if enabled && !available() {
            return Err(unsupported());
        }

        if enabled {
            let count = self.write_index(db).await?;
            self.persist(true, count)?;
        } else {
            clear_index()?;
            self.persist(false, 0)?;
        }

        Ok(self.status())
    }

    /// Rewrite the whole index from the current library. A no-op when the user
    /// has not opted in.
    pub async fn reindex(
        &self,
        db: &DatabaseConnection,
    ) -> Result<SpotlightStatus, SpotlightError> {
        let _guard = self.index_lock.lock().await;

        if !self.settings().enabled {
            return Ok(self.status());
        }
        if !available() {
            return Err(unsupported());
        }

        let count = self.write_index(db).await?;
        self.persist(true, count)?;
        Ok(self.status())
    }

    async fn write_index(&self, db: &DatabaseConnection) -> Result<usize, SpotlightError> {
        let records = load_records(db).await?;

        #[cfg(target_os = "macos")]
        {
            tauri::async_runtime::spawn_blocking(move || macos::index_records(&records))
                .await
                .map_err(|error| SpotlightError::new("index", error.to_string()))?
        }

        #[cfg(not(target_os = "macos"))]
        {
            let _ = records;
            Ok(0)
        }
    }

    fn persist(&self, enabled: bool, count: usize) -> Result<(), SpotlightError> {
        let settings = SpotlightSettings {
            enabled,
            indexed_at: enabled.then(now_rfc3339),
            item_count: count as u32,
        };
        self.store
            .write(&settings)
            .map_err(|error| SpotlightError::new("storage", error))
    }
}

fn available() -> bool {
    #[cfg(target_os = "macos")]
    {
        macos::is_available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

fn clear_index() -> Result<(), SpotlightError> {
    #[cfg(target_os = "macos")]
    {
        macos::clear_index()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

fn unsupported() -> SpotlightError {
    SpotlightError::new(
        "unsupported",
        "Spotlight indexing is only available on macOS",
    )
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Read every edition with the metadata Spotlight should match on.
///
/// The joins are loaded wholesale rather than per edition: a personal library
/// is small enough that one pass beats thousands of round trips.
async fn load_records(db: &DatabaseConnection) -> Result<Vec<SpotlightRecord>, SpotlightError> {
    let editions = editions::Entity::find()
        .select_only()
        .columns([
            editions::Column::Id,
            editions::Column::WorkId,
            editions::Column::Title,
            editions::Column::Description,
        ])
        .into_model::<EditionRow>()
        .all(db)
        .await
        .map_err(database)?;
    if editions.is_empty() {
        return Ok(Vec::new());
    }

    let works: HashMap<DbId, WorkRow> = works::Entity::find()
        .select_only()
        .columns([
            works::Column::Id,
            works::Column::Title,
            works::Column::Description,
        ])
        .into_model::<WorkRow>()
        .all(db)
        .await
        .map_err(database)?
        .into_iter()
        .map(|work| (work.id, work))
        .collect();

    let authors_by_edition = grouped_names(
        edition_authors::Entity::find()
            .select_only()
            .columns([
                edition_authors::Column::EditionId,
                edition_authors::Column::AuthorId,
            ])
            .into_model::<EditionAuthorRow>()
            .all(db)
            .await
            .map_err(database)?,
        authors::Entity::find()
            .select_only()
            .columns([authors::Column::Id, authors::Column::Name])
            .into_model::<NameRow>()
            .all(db)
            .await
            .map_err(database)?,
        |link| (link.edition_id, link.author_id),
        |author| (author.id, author.name.clone()),
    );

    let tags_by_edition = grouped_names(
        edition_tags::Entity::find()
            .select_only()
            .columns([edition_tags::Column::EditionId, edition_tags::Column::TagId])
            .into_model::<EditionTagRow>()
            .all(db)
            .await
            .map_err(database)?,
        tags::Entity::find()
            .select_only()
            .columns([tags::Column::Id, tags::Column::Name])
            .into_model::<NameRow>()
            .all(db)
            .await
            .map_err(database)?,
        |link| (link.edition_id, link.tag_id),
        |tag| (tag.id, tag.name.clone()),
    );

    let subjects_by_edition = grouped_names(
        edition_subjects::Entity::find()
            .select_only()
            .columns([
                edition_subjects::Column::EditionId,
                edition_subjects::Column::SubjectId,
            ])
            .into_model::<EditionSubjectRow>()
            .all(db)
            .await
            .map_err(database)?,
        subjects::Entity::find()
            .select_only()
            .columns([subjects::Column::Id, subjects::Column::Name])
            .into_model::<NameRow>()
            .all(db)
            .await
            .map_err(database)?,
        |link| (link.edition_id, link.subject_id),
        |subject| (subject.id, subject.name.clone()),
    );

    let identifiers_by_edition = grouped_names(
        edition_identifiers::Entity::find()
            .select_only()
            .columns([
                edition_identifiers::Column::EditionId,
                edition_identifiers::Column::IdentifierId,
            ])
            .into_model::<EditionIdentifierRow>()
            .all(db)
            .await
            .map_err(database)?,
        identifiers::Entity::find()
            .select_only()
            .columns([identifiers::Column::Id, identifiers::Column::Value])
            .into_model::<IdentifierRow>()
            .all(db)
            .await
            .map_err(database)?,
        |link| (link.edition_id, link.identifier_id),
        |identifier| (identifier.id, identifier.value.clone()),
    );

    let covers: HashMap<DbId, String> = digital_inventory::Entity::find()
        .select_only()
        .columns([
            digital_inventory::Column::EditionId,
            digital_inventory::Column::CoverPath,
        ])
        .into_model::<CoverRow>()
        .all(db)
        .await
        .map_err(database)?
        .into_iter()
        .filter_map(|row| row.cover_path.map(|path| (row.edition_id, path)))
        .collect();

    Ok(editions
        .into_iter()
        .map(|edition| {
            let work = works.get(&edition.work_id);
            let title = edition
                .title
                .clone()
                .or_else(|| work.map(|work| work.title.clone()))
                .unwrap_or_else(|| "Untitled".to_owned());
            let description = edition
                .description
                .clone()
                .or_else(|| work.and_then(|work| work.description.clone()));

            let mut keywords = Vec::new();
            keywords.extend(
                tags_by_edition
                    .get(&edition.id)
                    .cloned()
                    .unwrap_or_default(),
            );
            keywords.extend(
                subjects_by_edition
                    .get(&edition.id)
                    .cloned()
                    .unwrap_or_default(),
            );
            keywords.extend(
                identifiers_by_edition
                    .get(&edition.id)
                    .cloned()
                    .unwrap_or_default(),
            );

            SpotlightRecord {
                edition_id: edition.id.to_string(),
                title,
                authors: authors_by_edition
                    .get(&edition.id)
                    .cloned()
                    .unwrap_or_default(),
                description,
                keywords,
                cover_path: covers.get(&edition.id).cloned(),
            }
        })
        .collect())
}

/// Join link rows to names in one pass.
fn grouped_names<L, M, V>(
    links: Vec<L>,
    models: Vec<M>,
    link: impl Fn(&L) -> (DbId, DbId),
    extract: impl Fn(&M) -> (DbId, V),
) -> HashMap<DbId, Vec<V>>
where
    V: Clone,
{
    let names: HashMap<DbId, V> = models.iter().map(extract).collect();

    let mut grouped: HashMap<DbId, Vec<V>> = HashMap::new();
    for item in &links {
        let (edition_id, target_id) = link(item);
        if let Some(value) = names.get(&target_id) {
            grouped.entry(edition_id).or_default().push(value.clone());
        }
    }
    grouped
}

fn database(error: impl std::fmt::Display) -> SpotlightError {
    SpotlightError::new("index", error.to_string())
}
