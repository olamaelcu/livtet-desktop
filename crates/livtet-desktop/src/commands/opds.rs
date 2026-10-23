//! OPDS catalog subscriptions and browsing.
//!
//! Catalogs are user-managed (CRUD) and persisted in a Tauri plugin-store file
//! owned by Rust. Credentials live in the OS keyring (see [`crate::secrets`]),
//! never in that file and never returned to the frontend: list views expose
//! only the auth kind.
//!
//! Responses are flattened into specta-safe DTOs ([`OpdsFeed`]) rather than
//! passing `livtet_opds_types` wire structs across IPC: the upstream types carry
//! `u64` fields that `specta-typescript` refuses to export (BigInt precision).
//!
//! Fail-closed rules:
//! - Only `http`/`https` feed URLs are accepted.
//! - Creating or updating a catalog probes the feed before persisting.
//! - Credentials are refused over plain `http` unless the host is loopback, and
//!   are only attached to requests on the catalog's own origin.

use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use livtet_opds_client::{Client, OpdsAuth, default_catalogs};
use livtet_opds_types::{Feed, Link, Publication};
use livtet_types::DbId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tokio::io::AsyncWriteExt;
use url::{Host, Url};

use crate::commands::import::{ImportError, ImportMode, ImportOutcome, import_one};
use crate::error::OpdsError;
use crate::secrets::{CatalogSecret, SecretStore};
use crate::types::AppState;

const CATALOGS_KEY: &str = "catalogs";

/// True when a link relation denotes an acquisition, in either the OPDS 1.x
/// URI form (`http://opds-spec.org/acquisition[/...]`), the short form the
/// Atom parser normalizes to (`acquisition`, `open-access`, ...), or an OPDS
/// 2.0 alias (`download`, `borrow`, `buy`, `preview`, `subscribe`).
fn is_acquisition_rel(rel: &str) -> bool {
    matches!(
        rel,
        "acquisition"
            | "open-access"
            | "borrow"
            | "buy"
            | "sample"
            | "preview"
            | "subscribe"
            | "download"
    ) || rel == "http://opds-spec.org/acquisition"
        || rel.starts_with("http://opds-spec.org/acquisition/")
}

/// True when an acquisition relation is open-access (freely downloadable).
fn is_open_access_rel(rel: &str) -> bool {
    matches!(rel, "open-access" | "download")
        || rel == "http://opds-spec.org/acquisition/open-access"
}

/// How a request to a catalog authenticates. Never carries the secret itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsAuthKind {
    #[default]
    None,
    Basic,
    Bearer,
}

/// Frontend-supplied credentials for create/update. Exactly one of
/// `basic`/`bearer` is populated when `kind` says so.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct OpdsAuthInput {
    pub kind: OpdsAuthKind,
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
}

/// A subscribed catalog as seen by the frontend (no credentials).
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsCatalog {
    pub id: String,
    pub title: String,
    pub feed_url: String,
    pub auth_kind: OpdsAuthKind,
    pub created_at: String,
    pub updated_at: String,
}

/// A built-in catalog the user can subscribe to with one click.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsPreset {
    pub title: String,
    pub url: String,
    pub description: String,
    pub version: String,
    pub requires_auth: bool,
}

/// A sub-section of a feed the user can drill into.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsNavigation {
    pub title: String,
    pub href: Option<String>,
}

/// A feed item flattened for the browser view.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsPublication {
    pub title: String,
    pub identifier: Option<String>,
    pub authors: Vec<String>,
    pub summary: Option<String>,
    pub cover_url: Option<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub published: Option<String>,
    pub acquisition_href: Option<String>,
    pub acquisition_type: Option<String>,
}

/// A feed page flattened for the browser view.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OpdsFeed {
    pub title: String,
    pub identifier: Option<String>,
    pub navigation: Vec<OpdsNavigation>,
    pub publications: Vec<OpdsPublication>,
    pub next_href: Option<String>,
    pub has_search: bool,
}

/// Build the secret half of an auth input. The `none` kind carries no secret,
/// so it yields `None` and nothing is written to the keyring.
fn secret_from_input(input: OpdsAuthInput) -> Result<Option<CatalogSecret>, OpdsError> {
    match input.kind {
        OpdsAuthKind::None => Ok(None),
        OpdsAuthKind::Basic => {
            let username = input.username.unwrap_or_default();
            let password = input.password.unwrap_or_default();
            if username.is_empty() {
                return Err(OpdsError::invalid_input("basic auth requires a username"));
            }
            Ok(Some(CatalogSecret::Basic { username, password }))
        }
        OpdsAuthKind::Bearer => {
            let token = input.token.unwrap_or_default();
            if token.is_empty() {
                return Err(OpdsError::invalid_input("bearer auth requires a token"));
            }
            Ok(Some(CatalogSecret::Bearer { token }))
        }
    }
}

impl CatalogSecret {
    fn kind(&self) -> OpdsAuthKind {
        match self {
            Self::Basic { .. } => OpdsAuthKind::Basic,
            Self::Bearer { .. } => OpdsAuthKind::Bearer,
        }
    }
}

/// Convert a stored secret into the client-crate auth type.
fn client_auth(secret: &CatalogSecret) -> OpdsAuth {
    match secret {
        CatalogSecret::Basic { username, password } => OpdsAuth::Basic {
            username: username.clone(),
            password: password.clone(),
        },
        CatalogSecret::Bearer { token } => OpdsAuth::Bearer {
            access_token: token.clone(),
        },
    }
}

/// Catalog record as persisted by Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCatalog {
    id: String,
    title: String,
    feed_url: String,
    /// How the catalog authenticates; the secret itself lives in the keyring.
    #[serde(default)]
    auth_kind: OpdsAuthKind,
    created_at: String,
    updated_at: String,
}

impl From<&StoredCatalog> for OpdsCatalog {
    fn from(catalog: &StoredCatalog) -> Self {
        Self {
            id: catalog.id.clone(),
            title: catalog.title.clone(),
            feed_url: catalog.feed_url.clone(),
            auth_kind: catalog.auth_kind,
            created_at: catalog.created_at.clone(),
            updated_at: catalog.updated_at.clone(),
        }
    }
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn format_date(value: Option<time::OffsetDateTime>) -> Option<String> {
    value.and_then(|date| {
        date.format(&time::format_description::well_known::Rfc3339)
            .ok()
    })
}

/// Pick the best acquisition link for an item. Prefers open-access EPUB, then
/// any EPUB, then any open-access, then the first acquisition link.
fn preferred_acquisition(links: &[Link]) -> Option<&Link> {
    let is_open_access = |link: &Link| link.rel.iter().any(|rel| is_open_access_rel(rel));
    let is_epub = |link: &Link| link.type_ == "application/epub+zip";

    let acquisitions: Vec<&Link> = links
        .iter()
        .filter(|link| link.rel.iter().any(|rel| is_acquisition_rel(rel)))
        .collect();
    acquisitions
        .iter()
        .copied()
        .find(|link| is_open_access(link) && is_epub(link))
        .or_else(|| acquisitions.iter().copied().find(|link| is_epub(link)))
        .or_else(|| {
            acquisitions
                .iter()
                .copied()
                .find(|link| is_open_access(link))
        })
        .or_else(|| acquisitions.first().copied())
}

fn cover_url(publication: &Publication) -> Option<String> {
    publication
        .images
        .first()
        .or_else(|| {
            publication
                .links
                .iter()
                .find(|link| link.rel.iter().any(|rel| rel.ends_with("/image")))
        })
        .map(|link| link.href.clone())
}

fn map_publication(publication: &Publication) -> OpdsPublication {
    let metadata = &publication.metadata;
    let acquisition = preferred_acquisition(&publication.links);
    OpdsPublication {
        title: metadata.title.clone(),
        identifier: metadata.identifier.clone(),
        authors: metadata.authors.iter().map(|a| a.name.clone()).collect(),
        summary: metadata.description.clone(),
        cover_url: cover_url(publication),
        language: metadata.language.clone(),
        publisher: metadata.publisher.clone(),
        published: format_date(metadata.published),
        acquisition_href: acquisition.map(|link| link.href.clone()),
        acquisition_type: acquisition.map(|link| link.type_.clone()),
    }
}

fn map_feed(feed: &Feed) -> OpdsFeed {
    OpdsFeed {
        title: feed.metadata.title.clone(),
        identifier: feed.metadata.identifier.clone(),
        navigation: feed
            .navigation
            .iter()
            .map(|link| OpdsNavigation {
                title: link.title.clone().unwrap_or_else(|| link.href.clone()),
                href: Some(link.href.clone()),
            })
            .collect(),
        publications: feed.publications.iter().map(map_publication).collect(),
        next_href: feed.next_page().map(|link| link.href.clone()),
        has_search: feed.search_template().is_some(),
    }
}

/// Where the catalog list is persisted. Production uses the Tauri plugin-store
/// file; the seam lets the command logic run without a Tauri app.
trait CatalogStore: Send + Sync {
    /// The persisted catalog list, if one has ever been written.
    fn read(&self) -> Option<serde_json::Value>;
    /// Replace the persisted catalog list and flush it.
    fn write(&self, value: serde_json::Value) -> Result<(), OpdsError>;
}

impl CatalogStore for tauri_plugin_store::Store<tauri::Wry> {
    fn read(&self) -> Option<serde_json::Value> {
        self.get(CATALOGS_KEY)
    }

    fn write(&self, value: serde_json::Value) -> Result<(), OpdsError> {
        self.set(CATALOGS_KEY, value);
        self.save().map_err(OpdsError::storage)
    }
}

fn load_catalogs(store: &dyn CatalogStore) -> Result<Vec<StoredCatalog>, OpdsError> {
    let Some(value) = store.read() else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value).map_err(OpdsError::storage)
}

fn save_catalogs(store: &dyn CatalogStore, catalogs: &[StoredCatalog]) -> Result<(), OpdsError> {
    let value = serde_json::to_value(catalogs).map_err(OpdsError::storage)?;
    store.write(value)
}

fn find_catalog(store: &dyn CatalogStore, id: &str) -> Result<StoredCatalog, OpdsError> {
    load_catalogs(store)?
        .into_iter()
        .find(|catalog| catalog.id == id)
        .ok_or_else(|| OpdsError::NotFound { id: id.to_string() })
}

/// The parts of [`AppState`] the OPDS commands use.
struct OpdsDeps<'a> {
    catalogs: &'a dyn CatalogStore,
    secrets: &'a Arc<dyn SecretStore>,
    http: &'a reqwest::Client,
}

impl<'a> OpdsDeps<'a> {
    fn new(state: &'a AppState) -> Self {
        Self {
            catalogs: &*state.opds_store,
            secrets: &state.secrets,
            http: &state.opds_http,
        }
    }
}

/// Parse and validate a feed URL, rejecting anything but `http`/`https`.
fn validate_feed_url(raw: &str) -> Result<Url, OpdsError> {
    let url = Url::parse(raw.trim()).map_err(|error| OpdsError::InvalidUrl {
        message: error.to_string(),
    })?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(OpdsError::InvalidUrl {
            message: format!("unsupported scheme '{other}'"),
        }),
    }
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host() == b.host()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

/// Refuse to attach credentials to a plain-http request to a remote host.
fn ensure_secure_auth(url: &Url, kind: OpdsAuthKind) -> Result<(), OpdsError> {
    if kind == OpdsAuthKind::None {
        return Ok(());
    }
    if url.scheme() == "http" && !is_loopback(url) {
        return Err(OpdsError::InsecureCredentials {
            message: format!("refusing to send credentials over plain http to {url}"),
        });
    }
    Ok(())
}

/// Read a secret from the OS keyring on a blocking thread.
async fn store_get(
    secrets: Arc<dyn SecretStore>,
    id: String,
) -> Result<Option<CatalogSecret>, OpdsError> {
    tokio::task::spawn_blocking(move || secrets.get(&id))
        .await
        .map_err(OpdsError::storage)?
        .map_err(OpdsError::from)
}

async fn store_set(
    secrets: Arc<dyn SecretStore>,
    id: String,
    secret: CatalogSecret,
) -> Result<(), OpdsError> {
    tokio::task::spawn_blocking(move || secrets.set(&id, &secret))
        .await
        .map_err(OpdsError::storage)?
        .map_err(OpdsError::from)
}

async fn store_delete(secrets: Arc<dyn SecretStore>, id: String) -> Result<(), OpdsError> {
    tokio::task::spawn_blocking(move || secrets.delete(&id))
        .await
        .map_err(OpdsError::storage)?
        .map_err(OpdsError::from)
}

/// Resolve a catalog's credentials from the keyring. Fail closed when the
/// catalog declares an auth kind but no secret is stored.
async fn resolve_auth(
    catalog: &StoredCatalog,
    secrets: &Arc<dyn SecretStore>,
) -> Result<OpdsAuth, OpdsError> {
    if catalog.auth_kind == OpdsAuthKind::None {
        return Ok(OpdsAuth::None);
    }
    let secret = store_get(secrets.clone(), catalog.id.clone())
        .await?
        .ok_or_else(|| OpdsError::CredentialsUnavailable {
            message: format!("no stored credentials for catalog {}", catalog.id),
        })?;
    Ok(client_auth(&secret))
}

async fn client_for(
    catalog: &StoredCatalog,
    http: &reqwest::Client,
    secrets: &Arc<dyn SecretStore>,
) -> Result<Client, OpdsError> {
    let auth = resolve_auth(catalog, secrets).await?;
    let client =
        Client::with_http(catalog.feed_url.clone(), http.clone()).map_err(OpdsError::from)?;
    Ok(client.auth(auth))
}

/// Fetch and flatten a catalog's root feed, proving the URL and credentials work.
async fn probe_feed(
    catalog_url: &Url,
    auth: OpdsAuth,
    http: &reqwest::Client,
) -> Result<OpdsFeed, OpdsError> {
    let client =
        Client::with_http(catalog_url.to_string(), http.clone()).map_err(OpdsError::from)?;
    client
        .auth(auth)
        .fetch_feed("")
        .await
        .map(|feed| map_feed(&feed))
        .map_err(OpdsError::from)
}

fn extension_for(url: &Url, content_type: Option<&str>) -> Option<&'static str> {
    if let Some(content_type) = content_type {
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match mime.as_str() {
            "application/epub+zip" | "application/x-mobipocket-ebook" => return Some("epub"),
            "application/pdf" => return Some("pdf"),
            _ => {}
        }
    }
    match url
        .path()
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("epub") => Some("epub"),
        Some("pdf") => Some("pdf"),
        _ => None,
    }
}

#[tauri::command]
#[specta::specta]
pub fn opds_default_catalogs() -> Vec<OpdsPreset> {
    default_catalogs()
        .into_iter()
        .map(|preset| OpdsPreset {
            title: preset.title,
            url: preset.url,
            description: preset.description,
            version: preset.version,
            requires_auth: preset.requires_auth,
        })
        .collect()
}

#[tauri::command]
#[specta::specta]
pub fn opds_catalogs_list(state: State<'_, AppState>) -> Result<Vec<OpdsCatalog>, OpdsError> {
    Ok(load_catalogs(&*state.opds_store)?
        .iter()
        .map(OpdsCatalog::from)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub async fn opds_catalogs_create(
    title: String,
    feed_url: String,
    auth: OpdsAuthInput,
    state: State<'_, AppState>,
) -> Result<OpdsCatalog, OpdsError> {
    opds_catalogs_create_inner(&OpdsDeps::new(&state), title, feed_url, auth).await
}

async fn opds_catalogs_create_inner(
    deps: &OpdsDeps<'_>,
    title: String,
    feed_url: String,
    auth: OpdsAuthInput,
) -> Result<OpdsCatalog, OpdsError> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(OpdsError::invalid_input("catalog title is required"));
    }
    let url = validate_feed_url(&feed_url)?;
    let secret = secret_from_input(auth)?;
    let kind = secret
        .as_ref()
        .map_or(OpdsAuthKind::None, CatalogSecret::kind);
    ensure_secure_auth(&url, kind)?;
    let probe_auth = secret.as_ref().map_or(OpdsAuth::None, client_auth);
    probe_feed(&url, probe_auth, deps.http).await?;

    let id = DbId::new().to_string();
    if let Some(secret) = &secret {
        store_set(deps.secrets.clone(), id.clone(), secret.clone()).await?;
    }
    let mut catalogs = load_catalogs(deps.catalogs)?;
    let now = now_rfc3339();
    let catalog = StoredCatalog {
        id,
        title,
        feed_url: url.to_string(),
        auth_kind: kind,
        created_at: now.clone(),
        updated_at: now,
    };
    catalogs.push(catalog.clone());
    save_catalogs(deps.catalogs, &catalogs)?;
    Ok(OpdsCatalog::from(&catalog))
}

#[tauri::command]
#[specta::specta]
pub async fn opds_catalogs_update(
    id: String,
    title: Option<String>,
    feed_url: Option<String>,
    auth: Option<OpdsAuthInput>,
    state: State<'_, AppState>,
) -> Result<OpdsCatalog, OpdsError> {
    opds_catalogs_update_inner(&OpdsDeps::new(&state), id, title, feed_url, auth).await
}

async fn opds_catalogs_update_inner(
    deps: &OpdsDeps<'_>,
    id: String,
    title: Option<String>,
    feed_url: Option<String>,
    auth: Option<OpdsAuthInput>,
) -> Result<OpdsCatalog, OpdsError> {
    let mut catalogs = load_catalogs(deps.catalogs)?;
    let index = catalogs
        .iter()
        .position(|catalog| catalog.id == id)
        .ok_or_else(|| OpdsError::NotFound { id: id.clone() })?;

    let mut catalog = catalogs[index].clone();

    if let Some(title) = title {
        let title = title.trim().to_string();
        if title.is_empty() {
            return Err(OpdsError::invalid_input("catalog title is required"));
        }
        catalog.title = title;
    }

    let mut needs_probe = false;
    if let Some(feed_url) = feed_url {
        catalog.feed_url = validate_feed_url(&feed_url)?.to_string();
        needs_probe = true;
    }

    let mut pending_secret: Option<Option<CatalogSecret>> = None;
    if let Some(auth_input) = auth {
        pending_secret = Some(secret_from_input(auth_input)?);
        needs_probe = true;
    }

    if needs_probe {
        let url = validate_feed_url(&catalog.feed_url)?;
        let kind = match &pending_secret {
            Some(secret) => secret
                .as_ref()
                .map_or(OpdsAuthKind::None, CatalogSecret::kind),
            None => catalog.auth_kind,
        };
        ensure_secure_auth(&url, kind)?;
        let probe_auth = match &pending_secret {
            Some(Some(secret)) => client_auth(secret),
            Some(None) => OpdsAuth::None,
            None => resolve_auth(&catalog, deps.secrets).await?,
        };
        probe_feed(&url, probe_auth, deps.http).await?;
    }

    if let Some(secret) = pending_secret {
        match secret {
            Some(secret) => {
                store_set(deps.secrets.clone(), catalog.id.clone(), secret.clone()).await?;
                catalog.auth_kind = secret.kind();
            }
            None => {
                store_delete(deps.secrets.clone(), catalog.id.clone()).await?;
                catalog.auth_kind = OpdsAuthKind::None;
            }
        }
    }

    catalog.updated_at = now_rfc3339();
    catalogs[index] = catalog.clone();
    save_catalogs(deps.catalogs, &catalogs)?;
    Ok(OpdsCatalog::from(&catalog))
}

#[tauri::command]
#[specta::specta]
pub async fn opds_catalogs_remove(id: String, state: State<'_, AppState>) -> Result<(), OpdsError> {
    opds_catalogs_remove_inner(&OpdsDeps::new(&state), id).await
}

async fn opds_catalogs_remove_inner(deps: &OpdsDeps<'_>, id: String) -> Result<(), OpdsError> {
    let mut catalogs = load_catalogs(deps.catalogs)?;
    let Some(index) = catalogs.iter().position(|catalog| catalog.id == id) else {
        return Err(OpdsError::NotFound { id });
    };
    let removed = catalogs.remove(index);
    if removed.auth_kind != OpdsAuthKind::None {
        store_delete(deps.secrets.clone(), removed.id.clone()).await?;
    }
    save_catalogs(deps.catalogs, &catalogs)
}

#[tauri::command]
#[specta::specta]
pub async fn opds_catalogs_test(
    id: String,
    state: State<'_, AppState>,
) -> Result<OpdsFeed, OpdsError> {
    opds_catalogs_test_inner(&OpdsDeps::new(&state), &id).await
}

async fn opds_catalogs_test_inner(deps: &OpdsDeps<'_>, id: &str) -> Result<OpdsFeed, OpdsError> {
    let catalog = find_catalog(deps.catalogs, id)?;
    let url = validate_feed_url(&catalog.feed_url)?;
    ensure_secure_auth(&url, catalog.auth_kind)?;
    let auth = resolve_auth(&catalog, deps.secrets).await?;
    probe_feed(&url, auth, deps.http).await
}

#[tauri::command]
#[specta::specta]
pub async fn opds_feed(id: String, state: State<'_, AppState>) -> Result<OpdsFeed, OpdsError> {
    opds_feed_inner(&OpdsDeps::new(&state), &id).await
}

async fn opds_feed_inner(deps: &OpdsDeps<'_>, id: &str) -> Result<OpdsFeed, OpdsError> {
    let catalog = find_catalog(deps.catalogs, id)?;
    client_for(&catalog, deps.http, deps.secrets)
        .await?
        .fetch_feed("")
        .await
        .map(|feed| map_feed(&feed))
        .map_err(OpdsError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn opds_page(
    id: String,
    href: String,
    state: State<'_, AppState>,
) -> Result<OpdsFeed, OpdsError> {
    opds_page_inner(&OpdsDeps::new(&state), &id, &href).await
}

async fn opds_page_inner(deps: &OpdsDeps<'_>, id: &str, href: &str) -> Result<OpdsFeed, OpdsError> {
    if href.trim().is_empty() {
        return Err(OpdsError::invalid_input("page href is required"));
    }
    let catalog = find_catalog(deps.catalogs, id)?;
    let catalog_url = validate_feed_url(&catalog.feed_url)?;
    let target = catalog_url
        .join(href.trim())
        .map_err(|error| OpdsError::InvalidUrl {
            message: error.to_string(),
        })?;
    if !same_origin(&catalog_url, &target) {
        return Err(OpdsError::invalid_input(
            "page link points outside the catalog",
        ));
    }
    client_for(&catalog, deps.http, deps.secrets)
        .await?
        .fetch_feed(target.as_str())
        .await
        .map(|feed| map_feed(&feed))
        .map_err(OpdsError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn opds_search(
    id: String,
    query: String,
    state: State<'_, AppState>,
) -> Result<OpdsFeed, OpdsError> {
    opds_search_inner(&OpdsDeps::new(&state), &id, &query).await
}

async fn opds_search_inner(
    deps: &OpdsDeps<'_>,
    id: &str,
    query: &str,
) -> Result<OpdsFeed, OpdsError> {
    let catalog = find_catalog(deps.catalogs, id)?;
    let client = client_for(&catalog, deps.http, deps.secrets).await?;
    livtet_opds_client::search::search(&client, "", query)
        .await
        .map(|feed| map_feed(&feed))
        .map_err(OpdsError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn opds_acquire(
    id: String,
    item_url: String,
    state: State<'_, AppState>,
) -> Result<ImportOutcome, OpdsError> {
    let app = state.inner();
    // The library owns a copy: the downloaded temp file is transient.
    let import = move |path: Utf8PathBuf| async move {
        import_one(path.as_str(), ImportMode::Copy, app).await
    };
    opds_acquire_inner(&OpdsDeps::new(app), &app.books_dir, &id, &item_url, import).await
}

/// Download an acquisition link into `<books_dir>/.opds-tmp` and hand the file
/// to `import`. The temp file is removed whatever the import outcome.
async fn opds_acquire_inner<F, Fut>(
    deps: &OpdsDeps<'_>,
    books_dir: &Utf8Path,
    id: &str,
    item_url: &str,
    import: F,
) -> Result<ImportOutcome, OpdsError>
where
    F: FnOnce(Utf8PathBuf) -> Fut,
    Fut: Future<Output = Result<ImportOutcome, ImportError>>,
{
    let catalog = find_catalog(deps.catalogs, id)?;
    let catalog_url = validate_feed_url(&catalog.feed_url)?;
    let url = validate_feed_url(item_url)?;
    ensure_secure_auth(&url, catalog.auth_kind)?;

    // Credentials only ever travel to the catalog's own origin; a CDN-hosted
    // acquisition link gets the request without them.
    let mut request = deps.http.get(url.clone());
    if same_origin(&catalog_url, &url) {
        request = resolve_auth(&catalog, deps.secrets).await?.apply(request);
    }
    let mut response = request.send().await.map_err(OpdsError::network)?;
    if !response.status().is_success() {
        return Err(OpdsError::Http {
            status: i32::from(response.status().as_u16()),
        });
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let extension = extension_for(&url, content_type.as_deref())
        .ok_or_else(|| OpdsError::feed("acquisition link is not an EPUB or PDF"))?;

    let temp_dir = books_dir.join(".opds-tmp");
    fs_err::tokio::create_dir_all(&temp_dir)
        .await
        .map_err(OpdsError::network)?;
    let temp_path = temp_dir.join(format!("{}.{}", DbId::new(), extension));

    let mut file = fs_err::tokio::File::create(&temp_path)
        .await
        .map_err(OpdsError::network)?;
    let streamed = async {
        while let Some(chunk) = response.chunk().await.map_err(OpdsError::network)? {
            file.write_all(&chunk).await.map_err(OpdsError::network)?;
        }
        file.flush().await.map_err(OpdsError::network)
    }
    .await;

    if let Err(error) = streamed {
        let _ = fs_err::tokio::remove_file(&temp_path).await;
        return Err(error);
    }

    let outcome = import(temp_path.clone()).await;
    let _ = fs_err::tokio::remove_file(&temp_path).await;
    outcome.map_err(|error| OpdsError::Import {
        code: error.code.clone(),
        message: error.message.clone(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    fn link(type_: &str, rels: &[&str]) -> Link {
        Link::new(
            "https://example.test/book.epub",
            type_,
            rels.iter().map(|rel| rel.to_string()).collect(),
        )
    }

    #[test]
    fn validate_feed_url_accepts_http_and_https() {
        assert!(validate_feed_url("https://example.test/opds").is_ok());
        assert!(validate_feed_url("http://localhost:8080/opds").is_ok());
    }

    #[test]
    fn validate_feed_url_rejects_other_schemes() {
        assert!(validate_feed_url("file:///etc/passwd").is_err());
        assert!(validate_feed_url("ftp://example.test/opds").is_err());
        assert!(validate_feed_url("not a url").is_err());
    }

    #[test]
    fn same_origin_compares_scheme_host_and_port() {
        let base = Url::parse("https://example.test/opds").unwrap();
        assert!(same_origin(
            &base,
            &Url::parse("https://example.test/page/2").unwrap()
        ));
        assert!(!same_origin(
            &base,
            &Url::parse("http://example.test/page/2").unwrap()
        ));
        assert!(!same_origin(
            &base,
            &Url::parse("https://cdn.test/book.epub").unwrap()
        ));
        assert!(!same_origin(
            &base,
            &Url::parse("https://example.test:8443/x").unwrap()
        ));
    }

    #[test]
    fn credentials_refused_over_plain_http_but_allowed_on_loopback() {
        let basic = CatalogSecret::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        let kind = basic.kind();
        let remote = Url::parse("http://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&remote, kind).is_err());

        let local = Url::parse("http://127.0.0.1:8080/opds").unwrap();
        assert!(ensure_secure_auth(&local, kind).is_ok());

        let secure = Url::parse("https://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&secure, kind).is_ok());
    }

    #[test]
    fn no_credentials_never_triggers_insecure_refusal() {
        let remote = Url::parse("http://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&remote, OpdsAuthKind::None).is_ok());
    }

    #[test]
    fn loopback_covers_localhost_and_ipv6_but_not_other_hosts() {
        let loopback = |raw: &str| is_loopback(&Url::parse(raw).unwrap());
        assert!(loopback("http://LOCALHOST:8080/opds"));
        assert!(loopback("http://[::1]:8080/opds"));
        assert!(loopback("http://127.0.0.2/opds"));
        assert!(!loopback("http://[2001:db8::1]/opds"));
        assert!(!loopback("http://localhost.example.test/opds"));
        assert!(!loopback("http://10.0.0.1/opds"));
        // No host at all is never loopback.
        assert!(!loopback("mailto:someone@example.test"));
    }

    #[test]
    fn preferred_acquisition_prefers_open_access_epub() {
        let plain = link("application/epub+zip", &["acquisition"]);
        let open = link("application/epub+zip", &["open-access"]);
        let links = vec![plain, open];
        let picked = preferred_acquisition(&links).expect("acquisition");
        assert!(picked.rel.iter().any(|rel| is_open_access_rel(rel)));
    }

    #[test]
    fn preferred_acquisition_falls_back_to_any_epub() {
        let pdf = link("application/pdf", &["acquisition"]);
        let epub = link("application/epub+zip", &["acquisition"]);
        let links = vec![pdf, epub];
        let picked = preferred_acquisition(&links).expect("acquisition");
        assert_eq!(picked.type_, "application/epub+zip");
    }

    #[test]
    fn preferred_acquisition_matches_ia_full_uri_rels() {
        let pdf = link(
            "application/pdf",
            &["http://opds-spec.org/acquisition/open-access"],
        );
        let sample = link("text/html", &["http://opds-spec.org/acquisition/sample"]);
        let links = vec![sample, pdf];
        let picked = preferred_acquisition(&links).expect("acquisition");
        assert_eq!(picked.type_, "application/pdf");
    }

    #[test]
    fn preferred_acquisition_returns_none_without_acquisition_rel() {
        let links = vec![link(
            "application/epub+zip",
            &["http://opds-spec.org/image"],
        )];
        assert!(preferred_acquisition(&links).is_none());
    }

    #[test]
    fn acquisition_rels_accept_short_full_and_opds2_aliases() {
        assert!(is_acquisition_rel("acquisition"));
        assert!(is_acquisition_rel("open-access"));
        assert!(is_acquisition_rel("borrow"));
        assert!(is_acquisition_rel("http://opds-spec.org/acquisition"));
        assert!(is_acquisition_rel(
            "http://opds-spec.org/acquisition/open-access"
        ));
        assert!(is_acquisition_rel(
            "http://opds-spec.org/acquisition/sample"
        ));
        assert!(is_acquisition_rel("download"));
        assert!(is_acquisition_rel("preview"));
        assert!(!is_acquisition_rel("self"));
        assert!(!is_acquisition_rel("http://opds-spec.org/image"));
        assert!(!is_acquisition_rel("collection"));

        assert!(is_open_access_rel("open-access"));
        assert!(is_open_access_rel("download"));
        assert!(is_open_access_rel(
            "http://opds-spec.org/acquisition/open-access"
        ));
        assert!(!is_open_access_rel("borrow"));
        assert!(!is_open_access_rel(
            "http://opds-spec.org/acquisition/borrow"
        ));
    }

    #[test]
    fn map_feed_maps_compact_navigation_links() {
        let mut feed = Feed::new("Archive.org");
        feed.navigation.push(Link::new(
            "https://archive.org/services/opds/catalog?type=navigation&nav_key=page_ebooks",
            "application/opds+json",
            vec!["collection".to_string()],
        ));
        feed.navigation[0].title = Some("eBooks".to_string());

        let mapped = map_feed(&feed);
        assert_eq!(mapped.navigation.len(), 1);
        assert_eq!(mapped.navigation[0].title, "eBooks");
        assert_eq!(
            mapped.navigation[0].href.as_deref(),
            Some("https://archive.org/services/opds/catalog?type=navigation&nav_key=page_ebooks")
        );
    }

    #[test]
    fn extension_detection_prefers_content_type_then_path() {
        let no_ext = Url::parse("https://example.test/download/42").unwrap();
        assert_eq!(
            extension_for(&no_ext, Some("application/epub+zip; charset=binary")),
            Some("epub")
        );
        let with_ext = Url::parse("https://example.test/book.pdf").unwrap();
        assert_eq!(extension_for(&with_ext, None), Some("pdf"));
        assert_eq!(extension_for(&no_ext, Some("text/html")), None);
    }

    #[test]
    fn secret_from_input_requires_the_matching_secret() {
        let missing_username = OpdsAuthInput {
            kind: OpdsAuthKind::Basic,
            username: Some(String::new()),
            password: Some("p".into()),
            token: None,
        };
        assert!(secret_from_input(missing_username).is_err());

        let missing_token = OpdsAuthInput {
            kind: OpdsAuthKind::Bearer,
            username: None,
            password: None,
            token: None,
        };
        assert!(secret_from_input(missing_token).is_err());
    }

    #[test]
    fn secret_from_input_treats_none_kind_as_no_secret() {
        let none = OpdsAuthInput {
            kind: OpdsAuthKind::None,
            username: None,
            password: None,
            token: None,
        };
        assert_eq!(secret_from_input(none).unwrap(), None);
    }

    #[tokio::test]
    async fn resolve_auth_reads_the_keyring_and_fails_closed() {
        use crate::secrets::InMemorySecretStore;

        let secrets: Arc<dyn SecretStore> = Arc::new(InMemorySecretStore::new());
        let mut catalog = StoredCatalog {
            id: "01CATALOG".into(),
            title: "Catalog".into(),
            feed_url: "https://example.test/opds".into(),
            auth_kind: OpdsAuthKind::Basic,
            created_at: String::new(),
            updated_at: String::new(),
        };

        // Declares basic auth but no secret is stored: fail closed.
        let error = resolve_auth(&catalog, &secrets)
            .await
            .expect_err("missing secret should fail closed");
        assert!(
            matches!(error, OpdsError::CredentialsUnavailable { .. }),
            "got {error:?}"
        );

        secrets
            .set(
                "01CATALOG",
                &CatalogSecret::Basic {
                    username: "u".into(),
                    password: "p".into(),
                },
            )
            .unwrap();
        assert!(matches!(
            resolve_auth(&catalog, &secrets).await.unwrap(),
            OpdsAuth::Basic { .. }
        ));

        catalog.auth_kind = OpdsAuthKind::None;
        assert!(matches!(
            resolve_auth(&catalog, &secrets).await.unwrap(),
            OpdsAuth::None
        ));
    }

    #[test]
    fn pure_map_feed_maps_navigation_and_publications() {
        let feed = livtet_opds_types::Feed::new("Test");
        let mapped = super::map_feed(&feed);
        assert_eq!(mapped.title, "Test");
    }

    #[test]
    fn pure_secret_from_input_none_is_none() {
        let input = super::OpdsAuthInput {
            kind: super::OpdsAuthKind::None,
            username: None,
            password: None,
            token: None,
        };
        assert!(super::secret_from_input(input).unwrap().is_none());
    }

    // ---- Harness: in-memory catalog store, in-memory keyring, loopback feeds ----

    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::secrets::InMemorySecretStore;

    /// Stand-in for the plugin-store file.
    #[derive(Default)]
    struct MemoryCatalogs(Mutex<Option<serde_json::Value>>);

    impl CatalogStore for MemoryCatalogs {
        fn read(&self) -> Option<serde_json::Value> {
            self.0.lock().unwrap().clone()
        }

        fn write(&self, value: serde_json::Value) -> Result<(), OpdsError> {
            *self.0.lock().unwrap() = Some(value);
            Ok(())
        }
    }

    struct Harness {
        catalogs: MemoryCatalogs,
        secrets: Arc<dyn SecretStore>,
        http: reqwest::Client,
    }

    impl Harness {
        fn new() -> Self {
            Self {
                catalogs: MemoryCatalogs::default(),
                secrets: Arc::new(InMemorySecretStore::new()),
                http: reqwest::Client::builder().no_proxy().build().unwrap(),
            }
        }

        fn deps(&self) -> OpdsDeps<'_> {
            OpdsDeps {
                catalogs: &self.catalogs,
                secrets: &self.secrets,
                http: &self.http,
            }
        }

        fn stored(&self) -> Vec<StoredCatalog> {
            load_catalogs(&self.catalogs).unwrap()
        }

        fn seed(&self, catalog: StoredCatalog, secret: Option<CatalogSecret>) {
            if let Some(secret) = secret {
                self.secrets.set(&catalog.id, &secret).unwrap();
            }
            let mut all = self.stored();
            all.push(catalog);
            save_catalogs(&self.catalogs, &all).unwrap();
        }

        fn secret(&self, id: &str) -> Option<CatalogSecret> {
            self.secrets.get(id).unwrap()
        }
    }

    fn stored_catalog(id: &str, feed_url: &str, auth_kind: OpdsAuthKind) -> StoredCatalog {
        StoredCatalog {
            id: id.into(),
            title: format!("Catalog {id}"),
            feed_url: feed_url.into(),
            auth_kind,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn basic_secret() -> CatalogSecret {
        CatalogSecret::Basic {
            username: "u".into(),
            password: "p".into(),
        }
    }

    /// `Authorization` header value for [`basic_secret`] (`base64("u:p")`).
    const BASIC_HEADER: &str = "Basic dTpw";

    fn auth_input(kind: OpdsAuthKind) -> OpdsAuthInput {
        OpdsAuthInput {
            kind,
            username: Some("u".into()),
            password: Some("p".into()),
            token: Some("tok-123".into()),
        }
    }

    #[derive(Clone)]
    struct Reply {
        status: u16,
        content_type: &'static str,
        body: Vec<u8>,
    }

    fn ok(content_type: &'static str, body: impl Into<Vec<u8>>) -> Reply {
        Reply {
            status: 200,
            content_type,
            body: body.into(),
        }
    }

    fn status(status: u16) -> Reply {
        Reply {
            status,
            content_type: "text/plain",
            body: Vec::new(),
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Seen {
        uri: String,
        authorization: Option<String>,
    }

    #[derive(Clone)]
    struct FixtureState {
        routes: Arc<HashMap<String, Reply>>,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    #[poem::handler]
    fn fixture(
        req: &poem::Request,
        poem::web::Data(state): poem::web::Data<&FixtureState>,
    ) -> poem::Response {
        let uri = req.uri().path_and_query().map_or_else(
            || req.uri().path().to_string(),
            |pq| pq.as_str().to_string(),
        );
        state.seen.lock().unwrap().push(Seen {
            uri: uri.clone(),
            authorization: req.header("authorization").map(str::to_string),
        });
        let reply = state
            .routes
            .get(&uri)
            .cloned()
            .unwrap_or_else(|| status(404));
        poem::Response::builder()
            .status(poem::http::StatusCode::from_u16(reply.status).unwrap())
            .content_type(reply.content_type)
            .body(reply.body)
    }

    /// A loopback HTTP server answering exact `path?query` routes; anything
    /// else is a 404. Every request is recorded with its `Authorization`.
    struct FixtureServer {
        origin: String,
        seen: Arc<Mutex<Vec<Seen>>>,
    }

    impl FixtureServer {
        /// `routes` bodies may contain `{origin}`, replaced with this server's origin.
        async fn start(routes: Vec<(&str, Reply)>) -> Self {
            use poem::EndpointExt;

            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let origin = format!("http://{}", listener.local_addr().unwrap());
            listener.set_nonblocking(true).unwrap();
            let acceptor = poem::listener::TcpAcceptor::from_std(listener).unwrap();
            let routes: HashMap<String, Reply> = routes
                .into_iter()
                .map(|(path, mut reply)| {
                    let body = String::from_utf8_lossy(&reply.body).replace("{origin}", &origin);
                    reply.body = body.into_bytes();
                    (path.to_string(), reply)
                })
                .collect();
            let seen = Arc::new(Mutex::new(Vec::new()));
            let state = FixtureState {
                routes: Arc::new(routes),
                seen: seen.clone(),
            };
            tokio::spawn(poem::Server::new_with_acceptor(acceptor).run(fixture.data(state)));
            Self { origin, seen }
        }

        fn url(&self, path: &str) -> String {
            format!("{}{path}", self.origin)
        }

        fn seen(&self) -> Vec<Seen> {
            self.seen.lock().unwrap().clone()
        }
    }

    const ATOM: &str = "application/atom+xml;profile=opds-catalog;kind=navigation";
    const OPDS_JSON: &str = "application/opds+json";

    /// OPDS 1.x root: one navigation entry, one publication, a next link.
    const ATOM_ROOT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/terms/">
  <id>urn:livtet:test-root</id>
  <title>Test Library</title>
  <updated>2026-01-01T00:00:00Z</updated>
  <link rel="self" href="/opds" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <link rel="next" href="/opds?page=2" type="application/atom+xml;profile=opds-catalog;kind=navigation"/>
  <entry>
    <title>New Releases</title>
    <id>urn:livtet:new</id>
    <link rel="subsection" href="/opds/new" type="application/atom+xml;profile=opds-catalog;kind=acquisition"/>
  </entry>
  <entry>
    <title>Dune</title>
    <id>urn:isbn:9780441013593</id>
    <author><name>Frank Herbert</name></author>
    <summary>Desert planet.</summary>
    <dc:language>en</dc:language>
    <published>1965-08-01T00:00:00Z</published>
    <link rel="http://opds-spec.org/image" href="/covers/dune.jpg" type="image/jpeg"/>
    <link rel="http://opds-spec.org/acquisition" href="/books/dune.pdf" type="application/pdf"/>
    <link rel="http://opds-spec.org/acquisition/open-access" href="/books/dune.epub" type="application/epub+zip"/>
  </entry>
</feed>"#;

    /// OPDS 2.0 root advertising an absolute OpenSearch-style template.
    const JSON_ROOT: &str = r#"{
  "metadata": {"title": "JSON Library"},
  "links": [
    {"rel": "self", "href": "/opds2/root.json", "type": "application/opds+json"},
    {"rel": "search", "href": "{origin}/opds2/search{?query}", "type": "application/opds+json", "templated": true}
  ],
  "navigation": [
    {"rel": "current", "href": "/opds2/new.json", "type": "application/opds+json", "title": "New"}
  ]
}"#;

    /// OPDS 2.0 page: an EPUB-vs-open-access precedence case, a
    /// publication with no acquisition link, and a next link.
    const JSON_PAGE: &str = r#"{
  "metadata": {"title": "Page 2", "identifier": "urn:livtet:page-2"},
  "links": [
    {"rel": "next", "href": "/opds2/page3.json", "type": "application/opds+json"}
  ],
  "publications": [
    {
      "metadata": {
        "title": "Children of Dune",
        "identifier": "urn:isbn:9780593098257",
        "author": ["Frank Herbert", {"name": "Brian Herbert"}],
        "description": "Sequel.",
        "language": "en",
        "publisher": "Ace",
        "published": "1976-04-01T00:00:00Z"
      },
      "links": [
        {"rel": "http://opds-spec.org/acquisition/open-access", "href": "/free/children.pdf", "type": "application/pdf"},
        {"rel": "http://opds-spec.org/acquisition/borrow", "href": "/loans/children.epub", "type": "application/epub+zip"}
      ],
      "images": [{"rel": "cover", "href": "/covers/children.jpg", "type": "image/jpeg"}]
    },
    {
      "metadata": {"title": "Catalog Only"},
      "links": [
        {"rel": "http://opds-spec.org/image", "href": "/covers/only.png", "type": "image/png"}
      ]
    }
  ]
}"#;

    const JSON_SEARCH_RESULTS: &str = r#"{
  "metadata": {"title": "Results for dune messiah"},
  "links": [],
  "publications": [
    {
      "metadata": {"title": "Dune Messiah"},
      "links": [
        {"rel": "http://opds-spec.org/acquisition/open-access", "href": "/free/messiah.epub", "type": "application/epub+zip"}
      ]
    }
  ]
}"#;

    // ---- Catalog store ----

    #[test]
    fn legacy_catalog_records_without_auth_kind_load_as_unauthenticated() {
        let store = MemoryCatalogs::default();
        store
            .write(serde_json::json!([{
                "id": "01LEGACY",
                "title": "Old",
                "feed_url": "https://example.test/opds",
                "created_at": "2025-01-01T00:00:00Z",
                "updated_at": "2025-01-01T00:00:00Z"
            }]))
            .unwrap();
        let loaded = load_catalogs(&store).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].auth_kind, OpdsAuthKind::None);
    }

    #[test]
    fn malformed_catalog_store_is_a_storage_error() {
        let store = MemoryCatalogs::default();
        store.write(serde_json::json!({"not": "a list"})).unwrap();
        let error = load_catalogs(&store).expect_err("malformed store");
        assert!(matches!(error, OpdsError::Storage { .. }), "got {error:?}");
        assert!(matches!(
            find_catalog(&store, "any"),
            Err(OpdsError::Storage { .. })
        ));
    }

    #[test]
    fn every_default_catalog_is_subscribable() {
        // One-click subscribe feeds the preset URL to `validate_feed_url`.
        for preset in opds_default_catalogs() {
            assert!(
                validate_feed_url(&preset.url).is_ok(),
                "preset {} has unusable url {}",
                preset.title,
                preset.url
            );
        }
    }

    // ---- Create ----

    #[tokio::test]
    async fn create_without_auth_probes_and_persists_no_secret() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();

        let created = opds_catalogs_create_inner(
            &h.deps(),
            "  My Library  ".into(),
            server.url("/opds"),
            auth_input(OpdsAuthKind::None),
        )
        .await
        .unwrap();

        assert_eq!(created.title, "My Library");
        assert_eq!(created.auth_kind, OpdsAuthKind::None);
        let stored = h.stored();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, created.id);
        assert_eq!(stored[0].feed_url, server.url("/opds"));
        assert_eq!(h.secret(&created.id), None);
        assert_eq!(
            server.seen(),
            vec![Seen {
                uri: "/opds".into(),
                authorization: None
            }]
        );
    }

    #[tokio::test]
    async fn create_with_basic_auth_probes_with_it_and_stores_the_secret() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();

        let created = opds_catalogs_create_inner(
            &h.deps(),
            "Private".into(),
            server.url("/opds"),
            auth_input(OpdsAuthKind::Basic),
        )
        .await
        .unwrap();

        assert_eq!(created.auth_kind, OpdsAuthKind::Basic);
        assert_eq!(h.stored()[0].auth_kind, OpdsAuthKind::Basic);
        assert_eq!(h.secret(&created.id), Some(basic_secret()));
        assert_eq!(
            server.seen()[0].authorization.as_deref(),
            Some(BASIC_HEADER)
        );
    }

    #[tokio::test]
    async fn create_refuses_credentials_over_plain_http_to_a_remote_host() {
        let h = Harness::new();
        let error = opds_catalogs_create_inner(
            &h.deps(),
            "Remote".into(),
            "http://example.test/opds".into(),
            auth_input(OpdsAuthKind::Bearer),
        )
        .await
        .expect_err("insecure credentials");
        assert!(
            matches!(error, OpdsError::InsecureCredentials { .. }),
            "got {error:?}"
        );
        assert!(h.stored().is_empty());
    }

    #[tokio::test]
    async fn create_does_not_persist_when_the_probe_fails() {
        let server = FixtureServer::start(vec![("/opds", status(401))]).await;
        let h = Harness::new();
        let error = opds_catalogs_create_inner(
            &h.deps(),
            "Locked".into(),
            server.url("/opds"),
            auth_input(OpdsAuthKind::Bearer),
        )
        .await
        .expect_err("probe should fail");
        assert!(
            matches!(error, OpdsError::Http { status: 401 }),
            "got {error:?}"
        );
        assert!(h.stored().is_empty());
        assert_eq!(
            server.seen()[0].authorization.as_deref(),
            Some("Bearer tok-123")
        );
    }

    #[tokio::test]
    async fn create_rejects_a_blank_title_and_non_http_urls() {
        let h = Harness::new();
        let blank = opds_catalogs_create_inner(
            &h.deps(),
            "   ".into(),
            "https://example.test/opds".into(),
            auth_input(OpdsAuthKind::None),
        )
        .await
        .expect_err("blank title");
        assert!(
            matches!(blank, OpdsError::InvalidInput { .. }),
            "got {blank:?}"
        );

        let file = opds_catalogs_create_inner(
            &h.deps(),
            "Local".into(),
            "file:///etc/opds.xml".into(),
            auth_input(OpdsAuthKind::None),
        )
        .await
        .expect_err("file scheme");
        assert!(matches!(file, OpdsError::InvalidUrl { .. }), "got {file:?}");
        assert!(h.stored().is_empty());
    }

    // ---- Probe errors ----

    #[tokio::test]
    async fn probing_a_non_feed_page_is_a_feed_error() {
        let server = FixtureServer::start(vec![(
            "/index.html",
            ok("text/html", "<!doctype html><html><body>hi</body></html>"),
        )])
        .await;
        let h = Harness::new();
        let error = opds_catalogs_create_inner(
            &h.deps(),
            "Website".into(),
            server.url("/index.html"),
            auth_input(OpdsAuthKind::None),
        )
        .await
        .expect_err("html is not a feed");
        assert!(matches!(error, OpdsError::Feed { .. }), "got {error:?}");
        assert!(h.stored().is_empty());
    }

    #[tokio::test]
    async fn testing_an_unreachable_catalog_is_a_network_error() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let h = Harness::new();
        h.seed(
            stored_catalog(
                "01DOWN",
                &format!("http://127.0.0.1:{port}/opds"),
                OpdsAuthKind::None,
            ),
            None,
        );
        let error = opds_catalogs_test_inner(&h.deps(), "01DOWN")
            .await
            .expect_err("nothing listens");
        assert!(matches!(error, OpdsError::Network { .. }), "got {error:?}");
    }

    #[tokio::test]
    async fn testing_a_catalog_with_missing_credentials_fails_closed_without_a_request() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01NOSECRET", &server.url("/opds"), OpdsAuthKind::Basic),
            None,
        );

        let error = opds_catalogs_test_inner(&h.deps(), "01NOSECRET")
            .await
            .expect_err("no secret stored");
        assert!(
            matches!(error, OpdsError::CredentialsUnavailable { .. }),
            "got {error:?}"
        );
        assert!(server.seen().is_empty());

        h.secrets.set("01NOSECRET", &basic_secret()).unwrap();
        let feed = opds_catalogs_test_inner(&h.deps(), "01NOSECRET")
            .await
            .unwrap();
        assert_eq!(feed.title, "Test Library");
        assert_eq!(
            server.seen()[0].authorization.as_deref(),
            Some(BASIC_HEADER)
        );
    }

    #[tokio::test]
    async fn testing_a_stored_remote_http_catalog_with_credentials_is_refused() {
        let h = Harness::new();
        h.seed(
            stored_catalog("01REMOTE", "http://example.test/opds", OpdsAuthKind::Bearer),
            Some(CatalogSecret::Bearer {
                token: "tok".into(),
            }),
        );
        let error = opds_catalogs_test_inner(&h.deps(), "01REMOTE")
            .await
            .expect_err("insecure");
        assert!(
            matches!(error, OpdsError::InsecureCredentials { .. }),
            "got {error:?}"
        );
    }

    // ---- Update ----

    #[tokio::test]
    async fn update_switching_auth_to_none_deletes_the_stored_secret() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );

        let updated = opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            None,
            None,
            Some(auth_input(OpdsAuthKind::None)),
        )
        .await
        .unwrap();

        assert_eq!(updated.auth_kind, OpdsAuthKind::None);
        assert_eq!(h.stored()[0].auth_kind, OpdsAuthKind::None);
        assert_eq!(h.secret("01CAT"), None);
        // The probe already ran without credentials.
        assert_eq!(server.seen()[0].authorization, None);
    }

    #[tokio::test]
    async fn update_rotating_to_bearer_replaces_the_secret() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );

        opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            None,
            None,
            Some(auth_input(OpdsAuthKind::Bearer)),
        )
        .await
        .unwrap();

        assert_eq!(h.stored()[0].auth_kind, OpdsAuthKind::Bearer);
        assert_eq!(
            h.secret("01CAT"),
            Some(CatalogSecret::Bearer {
                token: "tok-123".into()
            })
        );
        assert_eq!(
            server.seen()[0].authorization.as_deref(),
            Some("Bearer tok-123")
        );
    }

    #[tokio::test]
    async fn update_title_only_does_not_probe() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::None),
            None,
        );

        let updated = opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            Some("  Renamed ".into()),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(updated.title, "Renamed");
        assert_eq!(h.stored()[0].title, "Renamed");
        assert!(server.seen().is_empty());
    }

    #[tokio::test]
    async fn update_feed_url_probes_with_the_stored_credentials() {
        let server = FixtureServer::start(vec![
            ("/opds", ok(ATOM, ATOM_ROOT)),
            ("/opds-moved", ok(ATOM, ATOM_ROOT)),
        ])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );

        let updated = opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            None,
            Some(server.url("/opds-moved")),
            None,
        )
        .await
        .unwrap();

        assert_eq!(updated.feed_url, server.url("/opds-moved"));
        assert_eq!(updated.auth_kind, OpdsAuthKind::Basic);
        assert_eq!(h.secret("01CAT"), Some(basic_secret()));
        assert_eq!(
            server.seen(),
            vec![Seen {
                uri: "/opds-moved".into(),
                authorization: Some(BASIC_HEADER.into())
            }]
        );
    }

    #[tokio::test]
    async fn update_with_a_failing_probe_leaves_the_catalog_and_secret_unchanged() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );

        let error = opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            Some("New title".into()),
            Some(server.url("/gone")),
            Some(auth_input(OpdsAuthKind::Bearer)),
        )
        .await
        .expect_err("probe 404s");
        assert!(
            matches!(error, OpdsError::Http { status: 404 }),
            "got {error:?}"
        );

        let stored = h.stored();
        assert_eq!(stored[0].feed_url, server.url("/opds"));
        assert_eq!(stored[0].title, "Catalog 01CAT");
        assert_eq!(stored[0].auth_kind, OpdsAuthKind::Basic);
        assert_eq!(h.secret("01CAT"), Some(basic_secret()));
    }

    #[tokio::test]
    async fn update_adding_credentials_to_a_remote_http_catalog_is_refused() {
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", "http://example.test/opds", OpdsAuthKind::None),
            None,
        );
        let error = opds_catalogs_update_inner(
            &h.deps(),
            "01CAT".into(),
            None,
            None,
            Some(auth_input(OpdsAuthKind::Basic)),
        )
        .await
        .expect_err("insecure");
        assert!(
            matches!(error, OpdsError::InsecureCredentials { .. }),
            "got {error:?}"
        );
        assert_eq!(h.secret("01CAT"), None);
        assert_eq!(h.stored()[0].auth_kind, OpdsAuthKind::None);
    }

    #[tokio::test]
    async fn update_rejects_unknown_ids_and_blank_titles() {
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", "https://example.test/opds", OpdsAuthKind::None),
            None,
        );
        let missing =
            opds_catalogs_update_inner(&h.deps(), "01NOPE".into(), None, None, None).await;
        assert!(
            matches!(&missing, Err(OpdsError::NotFound { id }) if id == "01NOPE"),
            "got {missing:?}"
        );

        let blank =
            opds_catalogs_update_inner(&h.deps(), "01CAT".into(), Some("  ".into()), None, None)
                .await;
        assert!(
            matches!(blank, Err(OpdsError::InvalidInput { .. })),
            "got {blank:?}"
        );
        assert_eq!(h.stored()[0].title, "Catalog 01CAT");
    }

    // ---- Remove ----

    #[tokio::test]
    async fn remove_deletes_the_catalog_and_its_secret_only() {
        let h = Harness::new();
        h.seed(
            stored_catalog("01A", "https://a.test/opds", OpdsAuthKind::Basic),
            Some(basic_secret()),
        );
        h.seed(
            stored_catalog("01B", "https://b.test/opds", OpdsAuthKind::Bearer),
            Some(CatalogSecret::Bearer { token: "b".into() }),
        );

        opds_catalogs_remove_inner(&h.deps(), "01A".into())
            .await
            .unwrap();

        let ids: Vec<String> = h.stored().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["01B".to_string()]);
        assert_eq!(h.secret("01A"), None);
        assert_eq!(
            h.secret("01B"),
            Some(CatalogSecret::Bearer { token: "b".into() })
        );

        let missing = opds_catalogs_remove_inner(&h.deps(), "01A".into()).await;
        assert!(
            matches!(missing, Err(OpdsError::NotFound { .. })),
            "got {missing:?}"
        );
    }

    // ---- Browsing ----

    #[tokio::test]
    async fn root_feed_maps_opds1_navigation_publications_and_next_link() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::None),
            None,
        );

        let feed = opds_feed_inner(&h.deps(), "01CAT").await.unwrap();

        assert_eq!(feed.title, "Test Library");
        assert_eq!(feed.identifier.as_deref(), Some("urn:livtet:test-root"));
        assert_eq!(feed.navigation.len(), 1);
        assert_eq!(feed.navigation[0].title, "New Releases");
        assert_eq!(feed.navigation[0].href.as_deref(), Some("/opds/new"));
        assert_eq!(feed.next_href.as_deref(), Some("/opds?page=2"));
        assert!(!feed.has_search);

        assert_eq!(feed.publications.len(), 1);
        let dune = &feed.publications[0];
        assert_eq!(dune.title, "Dune");
        assert_eq!(dune.identifier.as_deref(), Some("urn:isbn:9780441013593"));
        assert_eq!(dune.authors, vec!["Frank Herbert".to_string()]);
        assert_eq!(dune.summary.as_deref(), Some("Desert planet."));
        assert_eq!(dune.language.as_deref(), Some("en"));
        assert_eq!(dune.published.as_deref(), Some("1965-08-01T00:00:00Z"));
        assert_eq!(dune.cover_url.as_deref(), Some("/covers/dune.jpg"));
        // Open-access EPUB beats the plain PDF acquisition listed first.
        assert_eq!(dune.acquisition_href.as_deref(), Some("/books/dune.epub"));
        assert_eq!(
            dune.acquisition_type.as_deref(),
            Some("application/epub+zip")
        );
    }

    #[tokio::test]
    async fn page_resolves_relative_hrefs_and_maps_opds2_publications() {
        let server = FixtureServer::start(vec![
            ("/opds2/root.json", ok(OPDS_JSON, JSON_ROOT)),
            ("/opds2/page2.json", ok(OPDS_JSON, JSON_PAGE)),
        ])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog(
                "01CAT",
                &server.url("/opds2/root.json"),
                OpdsAuthKind::Bearer,
            ),
            Some(CatalogSecret::Bearer {
                token: "tok-123".into(),
            }),
        );

        let page = opds_page_inner(&h.deps(), "01CAT", " page2.json ")
            .await
            .unwrap();

        assert_eq!(
            server.seen(),
            vec![Seen {
                uri: "/opds2/page2.json".into(),
                authorization: Some("Bearer tok-123".into())
            }]
        );
        assert_eq!(page.title, "Page 2");
        assert_eq!(page.next_href.as_deref(), Some("/opds2/page3.json"));
        assert_eq!(page.publications.len(), 2);

        let children = &page.publications[0];
        assert_eq!(
            children.authors,
            vec!["Frank Herbert".to_string(), "Brian Herbert".to_string()]
        );
        assert_eq!(children.publisher.as_deref(), Some("Ace"));
        assert_eq!(children.published.as_deref(), Some("1976-04-01T00:00:00Z"));
        assert_eq!(children.cover_url.as_deref(), Some("/covers/children.jpg"));
        // Any EPUB beats an open-access non-EPUB.
        assert_eq!(
            children.acquisition_href.as_deref(),
            Some("/loans/children.epub")
        );

        let catalog_only = &page.publications[1];
        // No images array: the cover falls back to an `/image` link.
        assert_eq!(catalog_only.cover_url.as_deref(), Some("/covers/only.png"));
        // An image link is not an acquisition: nothing to download.
        assert_eq!(catalog_only.acquisition_href, None);
        assert_eq!(catalog_only.acquisition_type, None);
    }

    #[tokio::test]
    async fn page_refuses_blank_and_cross_origin_hrefs_without_a_request() {
        let server =
            FixtureServer::start(vec![("/opds2/root.json", ok(OPDS_JSON, JSON_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog(
                "01CAT",
                &server.url("/opds2/root.json"),
                OpdsAuthKind::Bearer,
            ),
            Some(CatalogSecret::Bearer {
                token: "tok-123".into(),
            }),
        );

        let blank = opds_page_inner(&h.deps(), "01CAT", "   ").await;
        assert!(
            matches!(blank, Err(OpdsError::InvalidInput { .. })),
            "got {blank:?}"
        );
        let foreign = opds_page_inner(&h.deps(), "01CAT", "https://elsewhere.test/feed.json").await;
        assert!(
            matches!(foreign, Err(OpdsError::InvalidInput { .. })),
            "got {foreign:?}"
        );
        let missing = opds_page_inner(&h.deps(), "01NOPE", "page2.json").await;
        assert!(
            matches!(missing, Err(OpdsError::NotFound { .. })),
            "got {missing:?}"
        );
        assert!(server.seen().is_empty());
    }

    #[tokio::test]
    async fn search_expands_the_template_with_the_encoded_query() {
        let server = FixtureServer::start(vec![
            ("/opds2/root.json", ok(OPDS_JSON, JSON_ROOT)),
            (
                "/opds2/search?query=dune+messiah",
                ok(OPDS_JSON, JSON_SEARCH_RESULTS),
            ),
        ])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds2/root.json"), OpdsAuthKind::None),
            None,
        );

        let root = opds_feed_inner(&h.deps(), "01CAT").await.unwrap();
        assert!(root.has_search);
        assert_eq!(root.navigation[0].title, "New");

        let results = opds_search_inner(&h.deps(), "01CAT", "dune messiah")
            .await
            .unwrap();
        assert_eq!(results.title, "Results for dune messiah");
        assert_eq!(results.publications[0].title, "Dune Messiah");
        let uris: Vec<String> = server.seen().into_iter().map(|s| s.uri).collect();
        assert_eq!(
            uris.last().map(String::as_str),
            Some("/opds2/search?query=dune+messiah")
        );
    }

    #[tokio::test]
    #[ignore = "bug: livtet-opds-client search::search strips the base path from the expanded \
                template and keeps only Url::path(), dropping the query string when the \
                catalog URL is a directory prefix of the search endpoint"]
    async fn search_keeps_the_query_when_the_catalog_url_is_a_directory() {
        let server = FixtureServer::start(vec![
            ("/opds2/", ok(OPDS_JSON, JSON_ROOT)),
            (
                "/opds2/search?query=dune+messiah",
                ok(OPDS_JSON, JSON_SEARCH_RESULTS),
            ),
        ])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds2/"), OpdsAuthKind::None),
            None,
        );
        let results = opds_search_inner(&h.deps(), "01CAT", "dune messiah")
            .await
            .unwrap();
        assert_eq!(results.title, "Results for dune messiah");
    }

    #[tokio::test]
    async fn search_without_a_template_is_a_feed_error() {
        let server = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::None),
            None,
        );
        let error = opds_search_inner(&h.deps(), "01CAT", "dune")
            .await
            .expect_err("no search template");
        assert!(matches!(error, OpdsError::Feed { .. }), "got {error:?}");
    }

    // ---- Acquire ----

    fn imported(edition_id: &str) -> ImportOutcome {
        ImportOutcome {
            work_id: "01WORK".into(),
            edition_id: edition_id.into(),
            title: "Dune".into(),
            isbns: Vec::new(),
            duplicate: false,
        }
    }

    fn refuse_import(path: Utf8PathBuf) -> std::future::Ready<Result<ImportOutcome, ImportError>> {
        panic!("import must not run, got {path}")
    }

    fn books_dir(root: &tempfile::TempDir) -> Utf8PathBuf {
        Utf8PathBuf::from(root.path().to_str().unwrap())
    }

    fn temp_files(books_dir: &Utf8Path) -> usize {
        match fs_err::read_dir(books_dir.join(".opds-tmp")) {
            Ok(entries) => entries.count(),
            Err(_) => 0,
        }
    }

    const EPUB_BYTES: &[u8] = b"PK\x03\x04 fake epub payload";

    #[tokio::test]
    async fn acquire_downloads_with_credentials_hands_the_file_to_import_and_cleans_up() {
        let server = FixtureServer::start(vec![(
            "/books/dune",
            ok("application/epub+zip; charset=binary", EPUB_BYTES),
        )])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );
        // The temp file path and its bytes as seen by the import step.
        type HandedFile = (Utf8PathBuf, Vec<u8>);
        let root = tempfile::tempdir().unwrap();
        let books = books_dir(&root);

        let handed: Arc<Mutex<Option<HandedFile>>> = Arc::default();
        let sink = handed.clone();
        let outcome = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            &server.url("/books/dune"),
            move |path: Utf8PathBuf| async move {
                let bytes = fs_err::tokio::read(&path).await.unwrap();
                *sink.lock().unwrap() = Some((path, bytes));
                Ok::<_, ImportError>(imported("01EDITION"))
            },
        )
        .await
        .unwrap();

        assert_eq!(outcome.edition_id, "01EDITION");
        let (path, bytes) = handed.lock().unwrap().take().expect("import ran");
        assert_eq!(bytes, EPUB_BYTES);
        // No extension in the URL: the content type decides.
        assert_eq!(path.extension(), Some("epub"));
        assert_eq!(path.parent(), Some(books.join(".opds-tmp").as_path()));
        assert!(!path.exists(), "temp file must be removed after import");
        assert_eq!(
            server.seen(),
            vec![Seen {
                uri: "/books/dune".into(),
                authorization: Some(BASIC_HEADER.into())
            }]
        );
    }

    #[tokio::test]
    async fn acquire_from_another_origin_never_sends_catalog_credentials() {
        let catalog = FixtureServer::start(vec![("/opds", ok(ATOM, ATOM_ROOT))]).await;
        let cdn = FixtureServer::start(vec![(
            "/cdn/dune.pdf",
            ok("application/octet-stream", "%PDF-1.7 fake"),
        )])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &catalog.url("/opds"), OpdsAuthKind::Basic),
            Some(basic_secret()),
        );
        let root = tempfile::tempdir().unwrap();
        let books = books_dir(&root);

        let extension: Arc<Mutex<Option<String>>> = Arc::default();
        let sink = extension.clone();
        opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            &cdn.url("/cdn/dune.pdf"),
            move |path: Utf8PathBuf| async move {
                *sink.lock().unwrap() = path.extension().map(str::to_string);
                Ok::<_, ImportError>(imported("01PDF"))
            },
        )
        .await
        .unwrap();

        // Generic content type: the URL path's extension decides.
        assert_eq!(extension.lock().unwrap().as_deref(), Some("pdf"));
        assert_eq!(
            cdn.seen(),
            vec![Seen {
                uri: "/cdn/dune.pdf".into(),
                authorization: None
            }]
        );
        assert!(catalog.seen().is_empty());
    }

    #[tokio::test]
    async fn acquire_maps_import_failures_and_still_removes_the_download() {
        let server = FixtureServer::start(vec![(
            "/books/broken.epub",
            ok("application/epub+zip", EPUB_BYTES),
        )])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::None),
            None,
        );
        let root = tempfile::tempdir().unwrap();
        let books = books_dir(&root);

        let error = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            &server.url("/books/broken.epub"),
            |_path: Utf8PathBuf| async {
                Err::<ImportOutcome, _>(ImportError::new("parse", "not a valid epub"))
            },
        )
        .await
        .expect_err("import fails");

        assert!(
            matches!(&error, OpdsError::Import { code, .. } if code == "parse"),
            "got {error:?}"
        );
        assert_eq!(temp_files(&books), 0);
    }

    #[tokio::test]
    async fn acquire_refuses_non_ebook_downloads_and_http_errors_without_importing() {
        let server = FixtureServer::start(vec![(
            "/books/landing",
            ok("text/html", "<html>buy me</html>"),
        )])
        .await;
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", &server.url("/opds"), OpdsAuthKind::None),
            None,
        );
        let root = tempfile::tempdir().unwrap();
        let books = books_dir(&root);

        let html = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            &server.url("/books/landing"),
            refuse_import,
        )
        .await;
        assert!(matches!(html, Err(OpdsError::Feed { .. })), "got {html:?}");

        let missing = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            &server.url("/books/gone.epub"),
            refuse_import,
        )
        .await;
        assert!(
            matches!(missing, Err(OpdsError::Http { status: 404 })),
            "got {missing:?}"
        );
        assert_eq!(temp_files(&books), 0);
    }

    #[tokio::test]
    async fn acquire_refuses_insecure_credentials_unknown_catalogs_and_bad_urls() {
        let h = Harness::new();
        h.seed(
            stored_catalog("01CAT", "http://example.test/opds", OpdsAuthKind::Basic),
            Some(basic_secret()),
        );
        let root = tempfile::tempdir().unwrap();
        let books = books_dir(&root);

        let insecure = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            "http://example.test/books/dune.epub",
            refuse_import,
        )
        .await;
        assert!(
            matches!(insecure, Err(OpdsError::InsecureCredentials { .. })),
            "got {insecure:?}"
        );

        let bad_scheme = opds_acquire_inner(
            &h.deps(),
            &books,
            "01CAT",
            "file:///etc/passwd",
            refuse_import,
        )
        .await;
        assert!(
            matches!(bad_scheme, Err(OpdsError::InvalidUrl { .. })),
            "got {bad_scheme:?}"
        );

        let unknown = opds_acquire_inner(
            &h.deps(),
            &books,
            "01NOPE",
            "https://example.test/books/dune.epub",
            refuse_import,
        )
        .await;
        assert!(
            matches!(unknown, Err(OpdsError::NotFound { .. })),
            "got {unknown:?}"
        );
    }
}
