//! KOReader progress sync (`kosync`) served from the desktop.
//!
//! KOReader's "Progress sync" plugin talks to a small HTTP service: it
//! authenticates with two headers (`x-auth-user`, `x-auth-key`, the latter the
//! MD5 of the password the reader typed), pushes a document's reading position
//! with `PUT /syncs/progress`, and pulls it back with
//! `GET /syncs/progress/:document`. This module is that service, running inside
//! the desktop over Poem — the same shape as the OPDS server
//! ([`crate::opds_server`]): a settings store, a runtime Settings can start,
//! stop and move, and a route table over the app's own SQLite pool.
//!
//! Two things make it Livtet's server rather than a generic relay.
//!
//! **Documents.** KOReader identifies a book by a digest of the file, not by any
//! catalog id: either the partial MD5 its `util.partialMD5` computes (1024-byte
//! samples at growing offsets) or, when the user chooses the "filename"
//! checksum method, the MD5 of the file name. `koreader_sync_ids` records which
//! digest belongs to which [`digital_inventory`](livtet_core::data::entities::digital_inventory)
//! row, computed from the library's own files, so a stock KOReader finds its
//! books without any pairing dialog.
//!
//! **Positions.** A computed position is written where the rest of the app
//! already reads it ([`crate::commands::reader`]): a PDF's page and an
//! audiobook's seconds go through the same writer the readers use, so opening
//! the book in Livtet resumes where KOReader stopped. A reflowable EPUB is the
//! one direction that cannot be translated — KOReader's position is an
//! `xpointer` and Livtet's is a Readium locator — so EPUB pushes keep their raw
//! string in `koreader_sync_state` and are relayed back to other KOReader
//! devices unchanged, while the percentage is mirrored into
//! `reading_progress.progress`.
//!
//! What is stored, and why it is desktop-owned: `livtet-core`'s migration set
//! ships as a git dependency and cannot grow a table here, the same constraint
//! that gave reviews their own table (ADR-0040). Both tables are created on
//! first use with `CREATE TABLE IF NOT EXISTS` and never touch a core migration.
//!
//! The server authenticates every route but `/healthcheck` and the
//! registration stub. Without a stored credential it refuses everything, so an
//! unpaired desktop serves nothing even on a shared network.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use livtet_core::data::entities::{digital_inventory, editions, reading_progress, works};
use livtet_core::data::orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait,
    IntoActiveModel, QueryFilter, Set, Statement, Value,
};
use livtet_types::{DbId, KnownFormats, now_primitive};
use md5::{Digest, Md5};
use poem::{
    EndpointExt, Route, get, handler,
    listener::Listener,
    middleware::AddData,
    post, put,
    web::{Data, Path},
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::Mutex;

use crate::commands::reader::{self, ReaderBookmark, ReaderPosition};
use crate::types::AppState;

/// Port the server binds when the user has not chosen one.
///
/// Next to the OPDS server's 8080 so the two never collide in a default setup.
pub(crate) const DEFAULT_PORT: u16 = 8081;

/// Key the settings are stored under inside `koreader-sync.json`.
pub(crate) const SETTINGS_KEY: &str = "koreader-sync";

/// Header KOReader sends the account name in.
const AUTH_HEADER_USER: &str = "x-auth-user";

/// Header KOReader sends the MD5 of the password in.
const AUTH_HEADER_KEY: &str = "x-auth-key";

/// How many library files one pass of [`map_documents`] may digest.
///
/// The sweep is bounded so a lookup that misses can advance it inside a request
/// without stalling the response on a large library; the startup pass simply
/// loops until the library is mapped.
const SWEEP_BATCH: usize = 128;

// ---------------------------------------------------------------- settings --

/// What Settings sends: the stored configuration plus a new password, if any.
///
/// The password is write-only. It is hashed into [`StoredKoreaderSync::userkey`]
/// on save and never stored or returned in the clear: KOReader itself only ever
/// sends the MD5 of what the user typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct KoreaderSyncSettings {
    /// Whether the server listens at all.
    pub enabled: bool,
    /// Port to bind.
    pub port: u16,
    /// Bind every interface (a tablet on the LAN is the client) rather than
    /// loopback only. Every route is credential-gated either way.
    pub lan: bool,
    /// Account name KOReader is configured with, sent as `x-auth-user`.
    pub username: Option<String>,
    /// A new password, hashed on save. `None` keeps the stored one; an empty
    /// string clears it and leaves the server unpaired.
    pub password: Option<String>,
}

impl Default for KoreaderSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            lan: true,
            username: None,
            password: None,
        }
    }
}

/// What is persisted: the configuration with the password already hashed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredKoreaderSync {
    pub enabled: bool,
    pub port: u16,
    pub lan: bool,
    pub username: Option<String>,
    /// MD5 hex of the password KOReader sends as `x-auth-key`.
    pub userkey: Option<String>,
}

impl Default for StoredKoreaderSync {
    fn default() -> Self {
        let baseline = KoreaderSyncSettings::default();
        Self {
            enabled: baseline.enabled,
            port: baseline.port,
            lan: baseline.lan,
            username: baseline.username,
            userkey: None,
        }
    }
}

impl StoredKoreaderSync {
    /// The view Settings renders: no secret, just whether one exists.
    pub(crate) fn view(&self) -> KoreaderSyncView {
        KoreaderSyncView {
            enabled: self.enabled,
            port: self.port,
            lan: self.lan,
            username: self.username.clone(),
            paired: self.credentials().is_some(),
        }
    }

    /// The credentials a request must carry, when the server is paired.
    fn credentials(&self) -> Option<Credentials> {
        let username = self.username.as_deref()?.trim();
        let userkey = self.userkey.as_deref()?.trim();
        if username.is_empty() || userkey.is_empty() {
            return None;
        }
        Some(Credentials {
            username: username.to_string(),
            userkey: userkey.to_ascii_lowercase(),
        })
    }

    /// Fold a Settings save into the stored configuration.
    ///
    /// A `None` password keeps the stored key; an empty one clears it, which
    /// unpairs the server. Saving never silently changes what an already
    /// configured KOReader sends.
    fn merged(&self, settings: &KoreaderSyncSettings) -> Self {
        let userkey = match settings.password.as_deref().map(str::trim) {
            Some("") => None,
            Some(password) => Some(md5_hex(password.as_bytes())),
            None => self.userkey.clone(),
        };
        Self {
            enabled: settings.enabled,
            port: settings.port,
            lan: settings.lan,
            username: settings
                .username
                .as_deref()
                .map(str::trim)
                .map(str::to_string),
            userkey,
        }
    }
}

/// The stored configuration without its secret, for the Settings panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Type)]
pub struct KoreaderSyncView {
    pub enabled: bool,
    pub port: u16,
    pub lan: bool,
    pub username: Option<String>,
    /// Whether a password is stored, i.e. whether KOReader can authenticate.
    pub paired: bool,
}

/// What Settings shows: the stored configuration plus what is actually bound.
#[derive(Debug, Clone, Serialize, Type)]
pub struct KoreaderSyncStatus {
    /// The persisted configuration.
    pub settings: KoreaderSyncView,
    /// Whether a listener is bound right now.
    pub running: bool,
    /// The base URL a KOReader device is configured with, when bound.
    pub url: Option<String>,
    /// How many books the library has mapped for KOReader, one per book even
    /// though each carries two digests.
    pub books: f64,
}

/// A server failure, shaped like the other command errors (`{ code, message }`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct KoreaderSyncError {
    /// Stable machine-readable reason: `invalid-port`, `unpaired`, `storage`,
    /// `bind`, `stop`, `database`, `not-found`, `invalid`, `unauthorized`.
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl KoreaderSyncError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The HTTP status this failure answers with.
    fn status(&self) -> poem::http::StatusCode {
        use poem::http::StatusCode;
        match self.code.as_str() {
            "unauthorized" | "unpaired" => StatusCode::UNAUTHORIZED,
            "not-found" => StatusCode::NOT_FOUND,
            "invalid" | "invalid-port" => StatusCode::BAD_REQUEST,
            // Upstream kosync answers 402 to a registration attempt; a local
            // server has exactly one account, so it is never created here.
            "registration-disabled" => StatusCode::PAYMENT_REQUIRED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<livtet_core::data::orm::DbErr> for KoreaderSyncError {
    fn from(error: livtet_core::data::orm::DbErr) -> Self {
        Self::new("database", error.to_string())
    }
}

impl From<reader::ReaderError> for KoreaderSyncError {
    fn from(error: reader::ReaderError) -> Self {
        Self::new(error.code, error.message)
    }
}

/// Where the settings live. The trait exists so tests and the `AppState`
/// fixture can supply an in-memory store instead of a Tauri store file.
pub(crate) trait KoreaderSyncSettingsStore: Send + Sync {
    /// The persisted settings, absent until the first save.
    fn read(&self) -> Option<StoredKoreaderSync>;
    /// Replace the persisted settings and flush them.
    fn write(&self, settings: &StoredKoreaderSync) -> Result<(), String>;
}

impl KoreaderSyncSettingsStore for tauri_plugin_store::Store<tauri::Wry> {
    fn read(&self) -> Option<StoredKoreaderSync> {
        self.get(SETTINGS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    fn write(&self, settings: &StoredKoreaderSync) -> Result<(), String> {
        let value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
        self.set(SETTINGS_KEY, value);
        self.save().map_err(|error| error.to_string())
    }
}

/// The credential pair a request must present.
#[derive(Clone)]
pub(crate) struct Credentials {
    username: String,
    userkey: String,
}

// ----------------------------------------------------------------- digests --

/// The digest KOReader's `util.partialMD5` computes for a file.
///
/// It reads 1024-byte samples at `1024 << (2 * i)` for `i` in `-1..=10` — the
/// negative exponent is a right shift, so the first sample starts at byte 256 —
/// and stops at the first offset that reads nothing. Every KOReader device
/// computes the same value for the same bytes, which is what makes a document
/// identifiable across them.
pub(crate) fn partial_md5(path: &Utf8Path) -> std::io::Result<String> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path)?;
    let mut hasher = Md5::new();
    let mut sample = [0u8; 1024];
    for i in -1i32..=10 {
        let shift = 2 * i;
        let offset = if shift >= 0 {
            1024u64 << shift
        } else {
            1024u64 >> -shift
        };
        file.seek(SeekFrom::Start(offset))?;
        let read = file.read(&mut sample)?;
        if read == 0 {
            break;
        }
        hasher.update(&sample[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// The digest KOReader computes when the user picks the "filename" checksum
/// method: the MD5 of the file's own name.
pub(crate) fn file_name_digest(name: &str) -> String {
    md5_hex(name.as_bytes())
}

fn md5_hex(bytes: &[u8]) -> String {
    let mut hasher = Md5::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

// ------------------------------------------------------------------ tables --

/// Create the two desktop-owned tables. Cheap and idempotent, so every entry
/// point calls it instead of relying on a migration that cannot exist here.
pub(crate) async fn ensure_tables(db: &DatabaseConnection) -> Result<(), KoreaderSyncError> {
    for statement in [
        // One row per KOReader document id. Two digests (partial MD5 and file
        // name MD5) may name the same file, so the index on the inventory row
        // is deliberately not unique.
        "CREATE TABLE IF NOT EXISTS koreader_sync_ids (
            id TEXT PRIMARY KEY NOT NULL,
            digital_inventory_id TEXT NOT NULL,
            edition_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT
        )",
        "CREATE INDEX IF NOT EXISTS idx_koreader_sync_ids_inventory_id
            ON koreader_sync_ids (digital_inventory_id)",
        "CREATE INDEX IF NOT EXISTS idx_koreader_sync_ids_edition_id
            ON koreader_sync_ids (edition_id)",
        // What the last push from each device carried, so a pull replays it to
        // the other devices unchanged, and `livtet_token` tells a later read
        // whether Livtet's own position has moved since.
        "CREATE TABLE IF NOT EXISTS koreader_sync_state (
            device_id TEXT NOT NULL,
            edition_id TEXT NOT NULL,
            device_name TEXT,
            percentage REAL NOT NULL DEFAULT 0,
            koreader_token TEXT,
            livtet_token TEXT,
            last_sync_at TEXT,
            PRIMARY KEY (device_id, edition_id)
        )",
        "CREATE INDEX IF NOT EXISTS idx_koreader_sync_state_edition_id
            ON koreader_sync_state (edition_id)",
    ] {
        db.execute_raw(Statement::from_string(
            DbBackend::Sqlite,
            statement.to_string(),
        ))
        .await?;
    }
    Ok(())
}

async fn count_mapped_books(db: &DatabaseConnection) -> Result<f64, KoreaderSyncError> {
    ensure_tables(db).await?;
    let row = db
        .query_one_raw(Statement::from_string(
            DbBackend::Sqlite,
            // Distinct editions, not rows: one book is mapped by both checksum
            // methods, and the count the Settings panel shows is books.
            "SELECT COUNT(DISTINCT edition_id) AS total FROM koreader_sync_ids".to_string(),
        ))
        .await?;
    let total = row
        .and_then(|row| row.try_get::<i64>("", "total").ok())
        .unwrap_or(0);
    Ok(total as f64)
}

// -------------------------------------------------------------- documents --

/// A KOReader document resolved to the catalog rows it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DocumentTarget {
    document: String,
    inventory_id: DbId,
    edition_id: DbId,
}

/// The catalog rows one document digest names, if the library has been mapped.
async fn lookup_document(
    db: &DatabaseConnection,
    document: &str,
) -> Result<Option<DocumentTarget>, KoreaderSyncError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id, digital_inventory_id, edition_id
                FROM koreader_sync_ids WHERE id = ?",
            [Value::from(document)],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let Ok(inventory_id) = row.try_get::<String>("", "digital_inventory_id") else {
        return Ok(None);
    };
    let Ok(edition_id) = row.try_get::<String>("", "edition_id") else {
        return Ok(None);
    };
    let (Ok(inventory_id), Ok(edition_id)) =
        (inventory_id.parse::<DbId>(), edition_id.parse::<DbId>())
    else {
        return Ok(None);
    };
    Ok(Some(DocumentTarget {
        document: document.to_string(),
        inventory_id,
        edition_id,
    }))
}

/// Digest up to `limit` library files that have no mapping yet, and record what
/// each digest names.
///
/// Only files inside the library store are read: a stored path that escapes it
/// (a hand-edited row, a stale import) is skipped rather than hashed. A file
/// the OS refuses to open is skipped too — one unreadable book must not stop
/// the sweep.
pub(crate) async fn map_documents(
    db: &DatabaseConnection,
    books_dir: &Utf8Path,
    limit: usize,
) -> Result<usize, KoreaderSyncError> {
    ensure_tables(db).await?;
    // Which inventory rows are already mapped is decided here, not in a join.
    // The catalog's id columns are 16-byte ULID blobs and this table stores the
    // same id as text; SQLite never compares those two as equal, so a join on
    // them matches nothing and every sweep re-hashed and re-inserted the whole
    // library — forever, because the sweep's loop only ends on an empty pass.
    let mapped: HashSet<String> = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT digital_inventory_id FROM koreader_sync_ids".to_string(),
        ))
        .await?
        .iter()
        .filter_map(|row| row.try_get::<String>("", "digital_inventory_id").ok())
        .collect();

    let rows = db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT i.id, i.edition_id, i.file_path
                FROM digital_inventory i
                WHERE i.file_path IS NOT NULL
                ORDER BY i.added_at ASC"
                .to_string(),
        ))
        .await?;

    let mut candidates = Vec::new();
    for row in rows {
        // The catalog's id columns are 16-byte BLOBs, so they decode as `DbId`
        // rather than as text; `koreader_sync_ids` is our own text.
        let (Ok(inventory_id), Ok(edition_id), Ok(file_path)) = (
            row.try_get::<DbId>("", "id"),
            row.try_get::<DbId>("", "edition_id"),
            row.try_get::<String>("", "file_path"),
        ) else {
            continue;
        };
        let path = Utf8PathBuf::from(file_path);
        if !reader::reader_path_in_library(books_dir, &path) {
            continue;
        }
        if mapped.contains(&inventory_id.to_string()) {
            continue;
        }
        candidates.push((inventory_id, edition_id, path));
        if candidates.len() >= limit {
            break;
        }
    }

    // Hashing is blocking file IO: keep it off the async workers.
    let digests = tokio::task::spawn_blocking(move || {
        let mut found = Vec::new();
        for (inventory_id, edition_id, path) in candidates {
            let Ok(partial) = partial_md5(&path) else {
                continue;
            };
            let mut names = vec![partial];
            if let Some(name) = path.file_name() {
                names.push(file_name_digest(name));
            }
            for document in names {
                found.push((document, inventory_id, edition_id));
            }
        }
        found
    })
    .await
    .map_err(|error| KoreaderSyncError::new("database", error.to_string()))?;

    let now = timestamp_text();
    let mut inserted = 0;
    for (document, inventory_id, edition_id) in &digests {
        let written = db
            .execute_raw(Statement::from_sql_and_values(
                DbBackend::Sqlite,
                "INSERT OR IGNORE INTO koreader_sync_ids
                (id, digital_inventory_id, edition_id, created_at)
                VALUES (?, ?, ?, ?)",
                [
                    Value::from(document.clone()),
                    Value::from(inventory_id.to_string()),
                    Value::from(edition_id.to_string()),
                    Value::from(now.clone()),
                ],
            ))
            .await?;
        inserted += written.rows_affected();
    }
    // Rows actually written, not rows visited: the caller loops until a sweep
    // maps nothing, and a visited-but-ignored row is no progress. That includes
    // a file whose digest another file already claimed, and a file that could
    // not be hashed — both stop the sweep instead of spinning on them.
    Ok(usize::try_from(inserted).unwrap_or(usize::MAX))
}

/// Resolve a document digest, mapping the library first when it is unknown.
///
/// A miss is the normal first-run path: the digest is only knowable by reading
/// the file, so one bounded sweep runs before answering "no such document".
pub(crate) async fn resolve_document(
    db: &DatabaseConnection,
    books_dir: &Utf8Path,
    document: &str,
) -> Result<Option<DocumentTarget>, KoreaderSyncError> {
    // The mapping table is created on first use, including inside a request:
    // nothing has run `ensure_tables` on a fresh install.
    ensure_tables(db).await?;
    if let Some(target) = lookup_document(db, document).await? {
        return Ok(Some(target));
    }
    if map_documents(db, books_dir, SWEEP_BATCH).await? == 0 {
        return Ok(None);
    }
    lookup_document(db, document).await
}

/// The edition behind an inventory row, for the `/koreader/sync/book/{id}`
/// endpoints that name a book by catalog id rather than by digest.
async fn target_for_inventory(
    db: &DatabaseConnection,
    inventory_id: DbId,
) -> Result<DocumentTarget, KoreaderSyncError> {
    ensure_tables(db).await?;
    let Some(inventory) = digital_inventory::Entity::find_by_id(inventory_id)
        .one(db)
        .await?
    else {
        return Err(KoreaderSyncError::new(
            "not-found",
            "no such library file in this catalog",
        ));
    };
    let document = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT id FROM koreader_sync_ids WHERE digital_inventory_id = ? LIMIT 1",
            [Value::from(inventory_id.to_string())],
        ))
        .await?
        .and_then(|row| row.try_get::<String>("", "id").ok())
        .unwrap_or_default();
    Ok(DocumentTarget {
        document,
        inventory_id,
        edition_id: inventory.edition_id,
    })
}

// --------------------------------------------------------------- positions --

/// The reader family a book's progress is stored in: `epub`, `pdf` or `audio`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Epub,
    Pdf,
    Audio,
}

impl Family {
    /// The `reading_progress.progress_unit` value this family writes.
    fn unit(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Audio => "timestamp",
        }
    }

    /// The `kind` the shared reader writer takes.
    fn kind(self) -> &'static str {
        match self {
            Self::Epub => "epub",
            Self::Pdf => "pdf",
            Self::Audio => "audio",
        }
    }
}

/// Which family a book's file belongs to.
fn family_for(edition: &editions::Model, file_format: Option<&str>) -> Family {
    let audiobook: DbId = KnownFormats::Audiobook.into();
    let pdf: DbId = KnownFormats::Pdf.into();
    // `digital_inventory.file_format` is the source file's extension, which is
    // the only format signal a catalog row imported without metadata carries.
    let audio_extensions = ["m4b", "m4a", "mp3"];
    if edition.format_id == Some(audiobook)
        || file_format.is_some_and(|extension| audio_extensions.contains(&extension))
    {
        Family::Audio
    } else if edition.format_id == Some(pdf) || file_format == Some("pdf") {
        Family::Pdf
    } else {
        Family::Epub
    }
}

/// The edition and the family it reads in.
async fn edition_family(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<(editions::Model, Family), KoreaderSyncError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Err(KoreaderSyncError::new("not-found", "edition not found"));
    };
    let file_format = digital_inventory::Entity::find()
        .filter(digital_inventory::Column::EditionId.eq(edition_id))
        .one(db)
        .await?
        .and_then(|inventory| inventory.file_format);
    let family = family_for(&edition, file_format.as_deref());
    Ok((edition, family))
}

/// Livtet's own stored position, as far as KOReader can express it.
#[derive(Debug, Clone, PartialEq)]
struct LivtetSnapshot {
    percentage: f64,
    /// A position KOReader can resume from: a page number, or playback seconds.
    progress: Option<String>,
    /// What the row looked like, so a later read can tell whether it moved.
    fingerprint: String,
}

/// Read Livtet's position and translate it into KOReader's terms.
async fn livtet_snapshot(
    db: &DatabaseConnection,
    edition_id: DbId,
    family: Family,
) -> Result<Option<LivtetSnapshot>, KoreaderSyncError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Ok(None);
    };
    let Some(format_id) = edition.format_id else {
        return Ok(None);
    };
    let Some(row) = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(format_id))
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let fingerprint = fingerprint_of(&row);
    let (percentage, progress) = match family {
        // A reflowable EPUB locator carries its place in the publication; the
        // locator itself is not something KOReader can resume from.
        Family::Epub => (
            epub_fraction(row.last_location.as_deref(), row.progress),
            None,
        ),
        Family::Pdf => {
            let page = row.progress.max(0.0);
            let total = edition
                .format_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("pages"))
                .and_then(serde_json::Value::as_f64)
                .filter(|total| *total > 0.0);
            (
                total.map_or(0.0, |total| (page / total).clamp(0.0, 1.0)),
                Some((page as u32).to_string()),
            )
        }
        Family::Audio => {
            let seconds = row.progress.max(0.0);
            let total = edition
                .format_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("duration_seconds"))
                .and_then(serde_json::Value::as_f64)
                .filter(|total| *total > 0.0);
            (
                total.map_or(0.0, |total| (seconds / total).clamp(0.0, 1.0)),
                Some(seconds.to_string()),
            )
        }
    };
    Ok(Some(LivtetSnapshot {
        percentage,
        progress,
        fingerprint,
    }))
}

/// The fraction of the publication a stored EPUB position names.
///
/// `locations.totalProgression` is the publication-wide number; `progression`
/// is only the place inside its own resource, so it is the fallback. The bare
/// `progress` column is the last resort — KOReader pushes write the fraction
/// there, and no other writer keeps it for the `epub` unit.
fn epub_fraction(locator: Option<&str>, stored: f64) -> f64 {
    if let Some(locator) = locator
        && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(locator)
        && let Some(locations) = parsed.get("locations")
    {
        let total = locations
            .get("totalProgression")
            .and_then(serde_json::Value::as_f64);
        let within = locations
            .get("progression")
            .and_then(serde_json::Value::as_f64);
        if let Some(fraction) = total.or(within) {
            return fraction.clamp(0.0, 1.0);
        }
    }
    if (0.0..=1.0).contains(&stored) {
        stored
    } else {
        0.0
    }
}

/// A cheap identity for a stored position, used to notice that it moved.
fn fingerprint_of(row: &reading_progress::Model) -> String {
    format!(
        "{}|{}|{}",
        row.progress_unit.as_deref().unwrap_or_default(),
        row.progress,
        row.last_location.as_deref().unwrap_or_default()
    )
}

/// What a pull answers: KOReader's own shape.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub(crate) struct SyncProgress {
    document: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<String>,
    percentage: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    device: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    timestamp: i64,
}

/// A state row: what the last push from one device carried.
#[derive(Debug, Clone)]
struct DeviceState {
    device_id: String,
    device_name: Option<String>,
    percentage: f64,
    koreader_token: Option<String>,
    livtet_token: Option<String>,
    last_sync_at: i64,
}

/// The newest push this edition has seen, whichever device sent it.
async fn newest_device_state(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<DeviceState>, KoreaderSyncError> {
    ensure_tables(db).await?;
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Sqlite,
            "SELECT device_id, device_name, percentage, koreader_token, livtet_token, last_sync_at
                FROM koreader_sync_state WHERE edition_id = ?
                ORDER BY last_sync_at DESC LIMIT 1",
            [Value::from(edition_id.to_string())],
        ))
        .await?
    else {
        return Ok(None);
    };
    Ok(Some(DeviceState {
        device_id: row.try_get("", "device_id").unwrap_or_default(),
        device_name: row.try_get("", "device_name").ok(),
        percentage: row.try_get("", "percentage").unwrap_or(0.0),
        koreader_token: row.try_get("", "koreader_token").ok(),
        livtet_token: row.try_get("", "livtet_token").ok(),
        last_sync_at: parse_timestamp(
            &row.try_get::<String>("", "last_sync_at")
                .unwrap_or_default(),
        ),
    }))
}

/// The position KOReader should resume from.
///
/// The newest device push wins, with one exception: when Livtet's own position
/// has moved since that push *and* is further along, Livtet wins, so reading in
/// the app on this machine is not silently undone by an old tablet push.
pub(crate) async fn read_sync_progress(
    db: &DatabaseConnection,
    target: &DocumentTarget,
) -> Result<Option<SyncProgress>, KoreaderSyncError> {
    let (_edition, family) = edition_family(db, target.edition_id).await?;
    let livtet = livtet_snapshot(db, target.edition_id, family).await?;
    let device = newest_device_state(db, target.edition_id).await?;
    match (device, livtet) {
        (Some(device), Some(livet)) => {
            let moved = device.livtet_token.as_deref() != Some(livet.fingerprint.as_str());
            if moved && livet.percentage > device.percentage {
                return Ok(Some(SyncProgress {
                    document: target.document.clone(),
                    progress: livet.progress,
                    percentage: livet.percentage,
                    device: None,
                    device_id: None,
                    timestamp: now_seconds(),
                }));
            }
            Ok(Some(SyncProgress {
                document: target.document.clone(),
                progress: device.koreader_token,
                percentage: device.percentage,
                device: device.device_name,
                device_id: Some(device.device_id),
                timestamp: device.last_sync_at,
            }))
        }
        (Some(device), None) => Ok(Some(SyncProgress {
            document: target.document.clone(),
            progress: device.koreader_token,
            percentage: device.percentage,
            device: device.device_name,
            device_id: Some(device.device_id),
            timestamp: device.last_sync_at,
        })),
        (None, Some(livet)) => Ok(Some(SyncProgress {
            document: target.document.clone(),
            progress: livet.progress,
            percentage: livet.percentage,
            device: None,
            device_id: None,
            timestamp: now_seconds(),
        })),
        (None, None) => Ok(None),
    }
}

/// A push from a KOReader device.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ProgressUpdate {
    document: String,
    progress: String,
    percentage: f64,
    device: String,
    #[serde(default)]
    device_id: Option<String>,
}

/// Store a push, and return the timestamp the answer reports.
pub(crate) async fn apply_progress(
    db: &DatabaseConnection,
    target: &DocumentTarget,
    update: &ProgressUpdate,
) -> Result<i64, KoreaderSyncError> {
    if !update.percentage.is_finite() {
        return Err(KoreaderSyncError::new(
            "invalid",
            "percentage must be finite",
        ));
    }
    if update.progress.trim().is_empty() {
        return Err(KoreaderSyncError::new(
            "invalid",
            "progress must not be empty",
        ));
    }
    if update.device.trim().is_empty() {
        return Err(KoreaderSyncError::new(
            "invalid",
            "device must not be empty",
        ));
    }
    // Every entry point bootstraps the tables it writes: a push can be the
    // first thing that ever runs on a fresh install.
    ensure_tables(db).await?;
    let (_edition, family) = edition_family(db, target.edition_id).await?;
    let percentage = update.percentage.clamp(0.0, 1.0);
    let trimmed = update.progress.trim();

    match family {
        // The one translation KOReader's `xpointer` cannot make: the locator
        // `reading_progress.last_location` holds belongs to Livtet's reader, so
        // only the fraction is mirrored across.
        Family::Epub => {
            store_epub_fraction(db, target.edition_id, percentage).await?;
        }
        Family::Pdf => {
            let page = trimmed.parse::<u32>().map_err(|_| {
                KoreaderSyncError::new("invalid", "a PDF position must be a page number")
            })?;
            reader::store_reader_position(
                db,
                target.edition_id,
                family.kind(),
                None,
                Some(page),
                None,
                None,
            )
            .await?;
        }
        Family::Audio => {
            let seconds = trimmed.parse::<f64>().map_err(|_| {
                KoreaderSyncError::new("invalid", "an audiobook position must be seconds")
            })?;
            reader::store_reader_position(
                db,
                target.edition_id,
                family.kind(),
                None,
                None,
                Some(seconds),
                None,
            )
            .await?;
        }
    }

    let fingerprint = livtet_snapshot(db, target.edition_id, family)
        .await?
        .map(|snapshot| snapshot.fingerprint);
    let now = timestamp_text();
    let device_id = update
        .device_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or(update.device.trim())
        .to_string();
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Sqlite,
        "INSERT INTO koreader_sync_state
            (device_id, edition_id, device_name, percentage, koreader_token,
             livtet_token, last_sync_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (device_id, edition_id) DO UPDATE SET
                device_name = excluded.device_name,
                percentage = excluded.percentage,
                koreader_token = excluded.koreader_token,
                livtet_token = excluded.livtet_token,
                last_sync_at = excluded.last_sync_at",
        [
            Value::from(device_id),
            Value::from(target.edition_id.to_string()),
            Value::from(update.device.trim().to_string()),
            Value::from(percentage),
            Value::from(trimmed.to_string()),
            Value::from(fingerprint),
            Value::from(now),
        ],
    ))
    .await?;
    Ok(now_seconds())
}

/// Mirror a KOReader percentage into an EPUB's `reading_progress` row.
///
/// Only `progress` is written: `last_location` is the reader's own locator and
/// `annotations` belongs to whoever owns the note list, so neither is touched.
async fn store_epub_fraction(
    db: &DatabaseConnection,
    edition_id: DbId,
    fraction: f64,
) -> Result<(), KoreaderSyncError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Err(KoreaderSyncError::new("not-found", "edition not found"));
    };
    let Some(format_id) = edition.format_id else {
        return Err(KoreaderSyncError::new(
            "invalid-format",
            "edition has no format_id",
        ));
    };
    let existing = reading_progress::Entity::find()
        .filter(reading_progress::Column::EditionId.eq(edition_id))
        .filter(reading_progress::Column::FormatId.eq(format_id))
        .one(db)
        .await?;
    match existing {
        Some(row) => {
            let mut active = row.into_active_model();
            active.progress = Set(fraction);
            active.progress_unit = Set(Some(Family::Epub.unit().to_string()));
            active.update(db).await?;
        }
        None => {
            reading_progress::ActiveModel {
                id: Set(DbId::new()),
                edition_id: Set(edition_id),
                format_id: Set(format_id),
                progress: Set(fraction),
                progress_unit: Set(Some(Family::Epub.unit().to_string())),
                last_location: Set(None),
                total_reading_time_secs: Set(0),
                annotations: Set(None),
                created_at: Set(now_primitive()),
            }
            .insert(db)
            .await?;
        }
    }
    Ok(())
}

// ------------------------------------------------- annotations & bookmarks --

/// Merge an incoming note list into the stored one, keyed by `Annotation.id`.
///
/// Notes Livtet holds that the push does not mention are kept: a device that
/// only knows about one chapter must not erase the rest. A note with the same
/// id is replaced, so an edited note wins without duplicating.
pub(crate) fn merge_annotations(
    stored: Option<&str>,
    incoming: &[serde_json::Value],
) -> Result<String, KoreaderSyncError> {
    let mut merged: Vec<serde_json::Value> = match stored {
        Some(raw) => serde_json::from_str::<Vec<serde_json::Value>>(raw).unwrap_or_default(),
        None => Vec::new(),
    };
    for note in incoming {
        let id = note
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        match id.and_then(|id| {
            merged.iter().position(|existing| {
                existing.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str())
            })
        }) {
            Some(index) => merged[index] = note.clone(),
            None => merged.push(note.clone()),
        }
    }
    serde_json::to_string(&merged)
        .map_err(|error| KoreaderSyncError::new("invalid", error.to_string()))
}

/// A book's position plus its notes and bookmarks, as the extension endpoints
/// exchange them.
#[derive(Debug, Clone, Serialize)]
struct BookSyncView {
    inventory_id: String,
    edition_id: String,
    document: String,
    title: Option<String>,
    progress: Option<ReaderPosition>,
    bookmarks: Vec<ReaderBookmark>,
}

/// Build the extension view for one inventory row.
async fn book_sync_view(
    db: &DatabaseConnection,
    target: &DocumentTarget,
) -> Result<BookSyncView, KoreaderSyncError> {
    let progress = reader::fetch_reader_position(db, target.edition_id).await?;
    let bookmarks = reader::fetch_reader_bookmarks(db, target.edition_id).await?;
    Ok(BookSyncView {
        inventory_id: target.inventory_id.to_string(),
        edition_id: target.edition_id.to_string(),
        document: target.document.clone(),
        title: title_for(db, target.edition_id).await?,
        progress,
        bookmarks,
    })
}

/// The edition's title, falling back to its work's when the edition has none.
async fn title_for(
    db: &DatabaseConnection,
    edition_id: DbId,
) -> Result<Option<String>, KoreaderSyncError> {
    let Some(edition) = editions::Entity::find_by_id(edition_id).one(db).await? else {
        return Ok(None);
    };
    if let Some(title) = edition.title.filter(|title| !title.trim().is_empty()) {
        return Ok(Some(title));
    }
    let work = works::Entity::find_by_id(edition.work_id).one(db).await?;
    Ok(work.map(|work| work.title))
}

/// A position pushed through the extension endpoint.
#[derive(Debug, Clone, Deserialize)]
struct ProgressPush {
    kind: String,
    #[serde(default)]
    locator: Option<String>,
    #[serde(default)]
    page_number: Option<u32>,
    #[serde(default)]
    position_seconds: Option<f64>,
    /// Notes to merge into the stored list, keyed by `id`.
    #[serde(default)]
    annotations: Option<Vec<serde_json::Value>>,
}

/// A bookmark pushed through the extension endpoint.
#[derive(Debug, Clone, Deserialize)]
struct BookmarkPush {
    kind: String,
    #[serde(default)]
    locator: Option<String>,
    #[serde(default)]
    page_number: Option<u32>,
    #[serde(default)]
    position_seconds: Option<f64>,
    #[serde(default)]
    fraction: Option<f64>,
}

/// One push to `/koreader/sync/book/{inventory_id}`.
#[derive(Debug, Clone, Default, Deserialize)]
struct BookSyncPush {
    #[serde(default)]
    progress: Option<ProgressPush>,
    #[serde(default)]
    bookmarks: Vec<BookmarkPush>,
    /// Bookmark ids to remove, so a deletion on one reading device reaches the
    /// others.
    #[serde(default)]
    delete_bookmarks: Vec<String>,
}

/// Apply a push: position first, then notes, then bookmark insertions and
/// deletions. Returns how many bookmarks exist afterwards.
async fn apply_book_sync(
    db: &DatabaseConnection,
    target: &DocumentTarget,
    push: &BookSyncPush,
) -> Result<usize, KoreaderSyncError> {
    // Position first: a bookmark's own write must land after it, so the notes
    // list and the bookmarks agree on where the reader is.
    if let Some(progress) = &push.progress {
        let stored = reader::fetch_reader_position(db, target.edition_id).await?;
        let annotations = match &progress.annotations {
            Some(incoming) => Some(merge_annotations(
                stored
                    .as_ref()
                    .and_then(|position| position.annotations.as_deref()),
                incoming,
            )?),
            None => None,
        };
        // A push that carries only notes keeps the position the reader wrote:
        // whichever half the payload omits falls back to what is stored, so a
        // merged highlight cannot reset the book to the start.
        reader::store_reader_position(
            db,
            target.edition_id,
            &progress.kind,
            progress.locator.clone().or_else(|| {
                stored
                    .as_ref()
                    .and_then(|position| position.locator.clone())
            }),
            progress
                .page_number
                .or_else(|| stored.as_ref().and_then(|position| position.page_number)),
            progress.position_seconds.or_else(|| {
                stored
                    .as_ref()
                    .and_then(|position| position.position_seconds)
            }),
            annotations,
        )
        .await?;
    }

    for id in &push.delete_bookmarks {
        if let Ok(bookmark_id) = id.parse::<DbId>() {
            reader::delete_reader_bookmark(db, bookmark_id).await?;
        }
    }

    let existing = reader::fetch_reader_bookmarks(db, target.edition_id).await?;
    for bookmark in &push.bookmarks {
        let duplicate = existing.iter().any(|stored| {
            stored.kind == bookmark.kind
                && stored.locator == bookmark.locator
                && stored.page_number == bookmark.page_number
                && stored.position_seconds == bookmark.position_seconds
        });
        if duplicate {
            continue;
        }
        reader::store_reader_bookmark(
            db,
            target.edition_id,
            &bookmark.kind,
            bookmark.locator.clone(),
            bookmark.page_number,
            bookmark.position_seconds,
            bookmark.fraction,
        )
        .await?;
    }

    Ok(reader::fetch_reader_bookmarks(db, target.edition_id)
        .await?
        .len())
}

// -------------------------------------------------------------------- HTTP --

/// Everything the routes read: the catalog pool, the library root, and the
/// credentials a request must present.
#[derive(Clone)]
pub(crate) struct SyncServerState {
    pub db: DatabaseConnection,
    pub books_dir: Utf8PathBuf,
    pub credentials: Option<Credentials>,
}

impl SyncServerState {
    /// Whether a request carries the configured credentials.
    ///
    /// Both halves must match, and an unpaired server accepts nothing: the
    /// empty-credential case is the fresh-install state, not a bypass.
    fn authorized(&self, req: &poem::Request) -> bool {
        let Some(expected) = &self.credentials else {
            return false;
        };
        let header = |name: &str| {
            req.headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        header(AUTH_HEADER_USER) == expected.username
            && header(AUTH_HEADER_KEY).eq_ignore_ascii_case(&expected.userkey)
    }
}

/// A JSON response.
fn json_response(status: poem::http::StatusCode, body: impl Serialize) -> poem::Response {
    match serde_json::to_vec(&body) {
        Ok(bytes) => poem::Response::builder()
            .status(status)
            .content_type("application/json")
            .body(bytes),
        Err(error) => poem::Response::builder()
            .status(poem::http::StatusCode::INTERNAL_SERVER_ERROR)
            .content_type("application/json")
            .body(format!(r#"{{"error":"{error}"}}"#)),
    }
}

/// The error response for a failure, in the shape KOReader only reads the
/// status of.
fn error_response(error: &KoreaderSyncError) -> poem::Response {
    json_response(
        error.status(),
        serde_json::json!({ "error": error.message, "code": error.code }),
    )
}

/// The route table KOReader talks to, plus the Livtet-side extension endpoints.
///
/// Paths outside `/koreader` are exactly KOReader's own: the plugin appends
/// them to whatever server URL the user typed, so they cannot be nested.
pub(crate) fn route(state: SyncServerState) -> impl poem::Endpoint<Output = poem::Response> {
    Route::new()
        // Unauthenticated on purpose: a health probe reveals nothing, and the
        // login screen needs to distinguish "wrong password" from "unreachable".
        .at("/healthcheck", get(healthcheck))
        .at("/users/auth", get(auth_user))
        .at("/users/create", post(create_user))
        .at("/syncs/progress", put(update_progress))
        .at("/syncs/progress/:document", get(get_progress))
        .at("/koreader/status", get(sync_status))
        .at("/koreader/books", get(sync_books))
        .at(
            "/koreader/sync/book/:inventory_id",
            get(book_sync).post(push_book_sync),
        )
        .with(AddData::new(state))
}

#[handler]
async fn healthcheck() -> poem::Response {
    json_response(
        poem::http::StatusCode::OK,
        serde_json::json!({ "state": "OK" }),
    )
}

/// The one account is configured in Settings, so authentication is a compare.
#[handler]
async fn auth_user(Data(state): Data<&SyncServerState>, req: &poem::Request) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    json_response(
        poem::http::StatusCode::OK,
        serde_json::json!({ "authorized": "OK" }),
    )
}

/// Registration is always refused: the account belongs to the desktop, and a
/// fresh password is a Settings action, not a remote one.
#[handler]
async fn create_user() -> poem::Response {
    error_response(&KoreaderSyncError::new(
        "registration-disabled",
        "accounts are created in Livtet's settings, not over sync",
    ))
}

#[handler]
async fn get_progress(
    Path(document): Path<String>,
    Data(state): Data<&SyncServerState>,
    req: &poem::Request,
) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    let target = match resolve_document(&state.db, &state.books_dir, &document).await {
        Ok(Some(target)) => target,
        Ok(None) => {
            return error_response(&KoreaderSyncError::new(
                "not-found",
                "no book in this library matches that document",
            ));
        }
        Err(error) => return error_response(&error),
    };
    match read_sync_progress(&state.db, &target).await {
        // Upstream answers an empty object for a document it has never seen.
        Ok(None) => json_response(
            poem::http::StatusCode::OK,
            serde_json::json!({ "document": document }),
        ),
        Ok(Some(progress)) => json_response(poem::http::StatusCode::OK, progress),
        Err(error) => error_response(&error),
    }
}

#[handler]
async fn update_progress(
    Data(state): Data<&SyncServerState>,
    body: String,
    req: &poem::Request,
) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    let update: ProgressUpdate = match serde_json::from_str(&body) {
        Ok(update) => update,
        Err(error) => {
            return error_response(&KoreaderSyncError::new(
                "invalid",
                format!("malformed progress payload: {error}"),
            ));
        }
    };
    let target = match resolve_document(&state.db, &state.books_dir, &update.document).await {
        Ok(Some(target)) => target,
        Ok(None) => {
            return error_response(&KoreaderSyncError::new(
                "not-found",
                "no book in this library matches that document",
            ));
        }
        Err(error) => return error_response(&error),
    };
    match apply_progress(&state.db, &target, &update).await {
        Ok(timestamp) => json_response(
            poem::http::StatusCode::OK,
            serde_json::json!({ "document": update.document, "timestamp": timestamp }),
        ),
        Err(error) => error_response(&error),
    }
}

#[handler]
async fn sync_status(Data(state): Data<&SyncServerState>, req: &poem::Request) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    match count_mapped_books(&state.db).await {
        Ok(books) => json_response(
            poem::http::StatusCode::OK,
            serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "books": books,
            }),
        ),
        Err(error) => error_response(&error),
    }
}

#[handler]
async fn sync_books(Data(state): Data<&SyncServerState>, req: &poem::Request) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    let rows = match state
        .db
        .query_all_raw(Statement::from_string(
            DbBackend::Sqlite,
            "SELECT k.id, k.digital_inventory_id, k.edition_id
                FROM koreader_sync_ids k
                ORDER BY k.created_at ASC"
                .to_string(),
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => return error_response(&KoreaderSyncError::from(error)),
    };
    let mut books = Vec::new();
    // One entry per book: a book carries two digests when both checksum
    // methods are mapped, and the panel lists books, not digests.
    let mut seen: Vec<DbId> = Vec::new();
    for row in rows {
        let (Ok(document), Ok(edition_id)) = (
            row.try_get::<String>("", "id"),
            row.try_get::<String>("", "edition_id"),
        ) else {
            continue;
        };
        let Ok(edition_id) = edition_id.parse::<DbId>() else {
            continue;
        };
        if seen.contains(&edition_id) {
            continue;
        }
        seen.push(edition_id);
        let (title, last_sync) = match (
            title_for(&state.db, edition_id).await,
            newest_device_state(&state.db, edition_id).await,
        ) {
            (Ok(title), Ok(state)) => (title, state.map(|state| state.last_sync_at)),
            (Err(error), _) | (_, Err(error)) => return error_response(&error),
        };
        books.push(serde_json::json!({
            "document": document,
            "edition_id": edition_id.to_string(),
            "title": title,
            "last_sync": last_sync,
        }));
    }
    json_response(poem::http::StatusCode::OK, books)
}

#[handler]
async fn book_sync(
    Path(inventory_id): Path<String>,
    Data(state): Data<&SyncServerState>,
    req: &poem::Request,
) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    let Ok(inventory_id) = inventory_id.parse::<DbId>() else {
        return error_response(&KoreaderSyncError::new(
            "invalid",
            "inventory id is invalid",
        ));
    };
    let target = match target_for_inventory(&state.db, inventory_id).await {
        Ok(target) => target,
        Err(error) => return error_response(&error),
    };
    match book_sync_view(&state.db, &target).await {
        Ok(view) => json_response(poem::http::StatusCode::OK, view),
        Err(error) => error_response(&error),
    }
}

#[handler]
async fn push_book_sync(
    Path(inventory_id): Path<String>,
    Data(state): Data<&SyncServerState>,
    body: String,
    req: &poem::Request,
) -> poem::Response {
    if !state.authorized(req) {
        return error_response(&unpaired_or_unauthorized(state));
    }
    let Ok(inventory_id) = inventory_id.parse::<DbId>() else {
        return error_response(&KoreaderSyncError::new(
            "invalid",
            "inventory id is invalid",
        ));
    };
    let push: BookSyncPush = match serde_json::from_str(&body) {
        Ok(push) => push,
        Err(error) => {
            return error_response(&KoreaderSyncError::new(
                "invalid",
                format!("malformed sync payload: {error}"),
            ));
        }
    };
    let target = match target_for_inventory(&state.db, inventory_id).await {
        Ok(target) => target,
        Err(error) => return error_response(&error),
    };
    match apply_book_sync(&state.db, &target, &push).await {
        Ok(bookmarks) => json_response(
            poem::http::StatusCode::OK,
            serde_json::json!({ "inventory_id": inventory_id.to_string(), "bookmarks": bookmarks }),
        ),
        Err(error) => error_response(&error),
    }
}

/// The failure for a request without credentials, which for an unpaired server
/// is a different story than a wrong password.
fn unpaired_or_unauthorized(state: &SyncServerState) -> KoreaderSyncError {
    if state.credentials.is_some() {
        KoreaderSyncError::new("unauthorized", "wrong account or password")
    } else {
        KoreaderSyncError::new(
            "unpaired",
            "set a username and password in Livtet's settings first",
        )
    }
}

// ----------------------------------------------------------------- runtime --

/// A bound listener.
struct Running {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<()>,
    url: String,
    host: IpAddr,
    port: u16,
}

/// The live listener plus the settings that drive it.
///
/// Held on [`AppState`] so Settings can start, stop, or move the listener
/// without restarting the app.
pub struct KoreaderSyncRuntime {
    store: Arc<dyn KoreaderSyncSettingsStore>,
    running: Mutex<Option<Running>>,
}

impl KoreaderSyncRuntime {
    /// Build the runtime over a settings store; nothing is bound until
    /// [`KoreaderSyncRuntime::apply`] runs.
    pub(crate) fn new(store: Arc<dyn KoreaderSyncSettingsStore>) -> Self {
        Self {
            store,
            running: Mutex::new(None),
        }
    }

    /// The persisted settings, or the off-by-default baseline.
    pub(crate) fn settings(&self) -> StoredKoreaderSync {
        self.store.read().unwrap_or_default()
    }

    /// The stored configuration plus what is actually bound.
    pub(crate) async fn status(&self, deps: &KoreaderSyncDeps) -> KoreaderSyncStatus {
        let running = self.running.lock().await;
        let settings = self.settings();
        KoreaderSyncStatus {
            settings: settings.view(),
            running: running.is_some(),
            url: running.as_ref().map(|running| running.url.clone()),
            books: count_mapped_books(&deps.db).await.unwrap_or(0.0),
        }
    }

    /// Reconcile the listener with `settings`, then persist them.
    ///
    /// A port of 0 is refused: it would bind an ephemeral port no reading
    /// device could be configured with. A refused save changes nothing —
    /// neither the bound listener nor the stored settings.
    pub(crate) async fn save(
        &self,
        settings: KoreaderSyncSettings,
        deps: &KoreaderSyncDeps,
    ) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
        if settings.port == 0 {
            return Err(KoreaderSyncError::new(
                "invalid-port",
                "port must be between 1 and 65535",
            ));
        }
        // Reach the requested configuration before remembering it: a port that
        // cannot bind must not cost the listener that is serving right now, and
        // must not be written to `koreader-sync.json`.
        let stored = self.settings().merged(&settings);
        self.apply(&stored, deps).await?;
        self.store
            .write(&stored)
            .map_err(|message| KoreaderSyncError::new("storage", message))?;
        Ok(self.status(deps).await)
    }

    /// Turn the server on at the stored port, and remember that choice.
    pub(crate) async fn start(
        &self,
        deps: &KoreaderSyncDeps,
    ) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
        let mut stored = self.settings();
        stored.enabled = true;
        self.apply(&stored, deps).await?;
        self.store
            .write(&stored)
            .map_err(|message| KoreaderSyncError::new("storage", message))?;
        Ok(self.status(deps).await)
    }

    /// Turn the server off, and remember that choice.
    pub(crate) async fn stop(
        &self,
        deps: &KoreaderSyncDeps,
    ) -> Result<KoreaderSyncStatus, KoreaderSyncError> {
        let mut stored = self.settings();
        stored.enabled = false;
        self.apply(&stored, deps).await?;
        self.store
            .write(&stored)
            .map_err(|message| KoreaderSyncError::new("storage", message))?;
        Ok(self.status(deps).await)
    }

    /// Reconcile the listener with `settings`, leaving it alone when it already
    /// serves the requested address.
    async fn apply(
        &self,
        settings: &StoredKoreaderSync,
        deps: &KoreaderSyncDeps,
    ) -> Result<(), KoreaderSyncError> {
        let desired = desired_address(settings);
        let mut running = self.running.lock().await;
        if let (Some((host, port)), Some(current)) = (desired, running.as_ref())
            && current.port == port
            && current.host == host
        {
            return Ok(());
        }
        let Some((host, port)) = desired else {
            if let Some(current) = running.take() {
                stop_running(current).await;
                tracing::info!("KOReader sync server stopped");
            }
            return Ok(());
        };

        // Bind the replacement first: a failed bind then leaves the listener
        // that is already serving untouched.
        ensure_tables(&deps.db).await?;
        let address = SocketAddr::new(host, port);
        // Bind before spawning: `TcpListener::bind` only records the address, so
        // the acceptor is where an already-taken port surfaces. Failing here
        // leaves the listener that is serving right now untouched.
        let acceptor = poem::listener::TcpListener::bind(address)
            .into_acceptor()
            .await
            .map_err(|error| KoreaderSyncError::new("bind", format!("bind {address}: {error}")))?;
        let state = SyncServerState {
            db: deps.db.clone(),
            books_dir: deps.books_dir.clone(),
            credentials: settings.credentials(),
        };
        let (shutdown, wait) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let result = poem::Server::new_with_acceptor(acceptor)
                .name("livtet-koreader")
                .run_with_graceful_shutdown(
                    route(state),
                    async {
                        let _ = wait.await;
                    },
                    Some(Duration::from_secs(5)),
                )
                .await;
            if let Err(error) = result {
                tracing::warn!(%error, "KOReader sync server task exited with an error");
            }
        });
        let url = base_url(settings, port);
        tracing::info!(%url, "KOReader sync server listening");

        if let Some(previous) = running.replace(Running {
            shutdown: Some(shutdown),
            task,
            url,
            host,
            port,
        }) {
            stop_running(previous).await;
        }
        Ok(())
    }
}

/// Ask a listener to stop, and wait for its task to finish.
async fn stop_running(running: Running) {
    if let Some(shutdown) = running.shutdown {
        let _ = shutdown.send(());
    }
    if let Err(error) = running.task.await {
        // The new listener is live and carries the requested configuration; a
        // join failure only means the old task did not shut down cleanly.
        tracing::warn!(%error, "a previous KOReader sync listener did not shut down cleanly");
    }
}

/// Start the server from persisted settings and map the library in the
/// background.
///
/// Called once during app setup. A failure is logged, not fatal: the app runs
/// without the server either way.
pub(crate) async fn start_from_settings(runtime: &KoreaderSyncRuntime, deps: &KoreaderSyncDeps) {
    let settings = runtime.settings();
    if let Err(error) = runtime.apply(&settings, deps).await {
        tracing::warn!(
            code = %error.code,
            message = %error.message,
            "KOReader sync server did not start"
        );
    }
    // Mapping reads every library file once. Doing it here keeps the first
    // KOReader request from paying for a whole library.
    let db = deps.db.clone();
    let books_dir = deps.books_dir.clone();
    tokio::spawn(async move {
        loop {
            match map_documents(&db, &books_dir, SWEEP_BATCH).await {
                Ok(0) => break,
                Ok(_) => tokio::task::yield_now().await,
                Err(error) => {
                    tracing::warn!(message = %error.message, "KOReader document mapping stopped");
                    break;
                }
            }
        }
    });
}

/// The address to bind, or `None` when the server should be off.
///
/// A stored port of 0 is treated as off: [`KoreaderSyncRuntime::save`] refuses
/// to write one, but a hand-edited store file must not bind a random port.
fn desired_address(settings: &StoredKoreaderSync) -> Option<(IpAddr, u16)> {
    if !settings.enabled || settings.port == 0 {
        return None;
    }
    let host = if settings.lan {
        IpAddr::V4(Ipv4Addr::UNSPECIFIED)
    } else {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    };
    Some((host, settings.port))
}

/// The base URL a KOReader device is configured with.
fn base_url(settings: &StoredKoreaderSync, port: u16) -> String {
    if !settings.lan {
        return format!("http://{}:{port}", Ipv4Addr::LOCALHOST);
    }
    match crate::opds_server::lan_ipv4() {
        Some(ip) => format!("http://{ip}:{port}"),
        None => format!("http://{}:{port}", Ipv4Addr::LOCALHOST),
    }
}

// ------------------------------------------------------------------- time --

/// RFC 3339 text for a stored timestamp.
fn timestamp_text() -> String {
    use time::format_description::well_known::Rfc3339;
    now_primitive()
        .assume_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| now_primitive().to_string())
}

/// Seconds since the Unix epoch, the unit KOReader's `timestamp` is in.
fn now_seconds() -> i64 {
    now_primitive().assume_utc().unix_timestamp()
}

/// Parse a stored RFC 3339 timestamp back into epoch seconds.
fn parse_timestamp(raw: &str) -> i64 {
    use time::format_description::well_known::Rfc3339;
    time::OffsetDateTime::parse(raw, &Rfc3339)
        .map(|parsed| parsed.unix_timestamp())
        .unwrap_or_else(|_| now_seconds())
}

// ------------------------------------------------------------------- deps --

/// The parts of [`AppState`] the server reads.
///
/// It deliberately excludes [`AppState::koreader_sync`]: the runtime owns the
/// listener, which owns the routes, which own this dependency set — holding the
/// whole `AppState` would close that loop.
#[derive(Clone)]
pub struct KoreaderSyncDeps {
    db: DatabaseConnection,
    books_dir: Utf8PathBuf,
}

impl KoreaderSyncDeps {
    /// Build the dependency set for a running app state.
    pub(crate) fn from_state(state: &AppState) -> Self {
        Self::new(state.db.db_conn(), state.books_dir.clone())
    }

    /// Build the dependency set from the two handles it reads.
    pub(crate) fn new(db: DatabaseConnection, books_dir: Utf8PathBuf) -> Self {
        Self { db, books_dir }
    }
}

#[cfg(test)]
mod tests;
