//! OPDS catalog subscriptions and browsing.
//!
//! Catalogs are user-managed (CRUD) and persisted in a Tauri plugin-store file
//! owned by Rust. Credentials live in the same store but are never returned to
//! the frontend: list views expose only the auth kind.
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

use livtet_opds_client::{Client, OpdsAuth, default_catalogs};
use livtet_opds_types::{Feed, Link, Publication};
use livtet_types::DbId;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;
use tokio::io::AsyncWriteExt;
use url::{Host, Url};

use crate::commands::import::{ImportMode, ImportOutcome, import_one};
use crate::error::OpdsError;
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum OpdsAuthKind {
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

/// Credentials as persisted (never serialized to the frontend).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StoredAuth {
    None,
    Basic { username: String, password: String },
    Bearer { token: String },
}

impl StoredAuth {
    fn from_input(input: OpdsAuthInput) -> Result<Self, OpdsError> {
        match input.kind {
            OpdsAuthKind::None => Ok(Self::None),
            OpdsAuthKind::Basic => {
                let username = input.username.unwrap_or_default();
                let password = input.password.unwrap_or_default();
                if username.is_empty() {
                    return Err(OpdsError::invalid_input("basic auth requires a username"));
                }
                Ok(Self::Basic { username, password })
            }
            OpdsAuthKind::Bearer => {
                let token = input.token.unwrap_or_default();
                if token.is_empty() {
                    return Err(OpdsError::invalid_input("bearer auth requires a token"));
                }
                Ok(Self::Bearer { token })
            }
        }
    }

    fn kind(&self) -> OpdsAuthKind {
        match self {
            Self::None => OpdsAuthKind::None,
            Self::Basic { .. } => OpdsAuthKind::Basic,
            Self::Bearer { .. } => OpdsAuthKind::Bearer,
        }
    }

    fn to_client_auth(&self) -> OpdsAuth {
        match self {
            Self::None => OpdsAuth::None,
            Self::Basic { username, password } => OpdsAuth::Basic {
                username: username.clone(),
                password: password.clone(),
            },
            Self::Bearer { token } => OpdsAuth::Bearer {
                access_token: token.clone(),
            },
        }
    }
}

/// Catalog record as persisted by Rust.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredCatalog {
    id: String,
    title: String,
    feed_url: String,
    auth: StoredAuth,
    created_at: String,
    updated_at: String,
}

impl From<&StoredCatalog> for OpdsCatalog {
    fn from(catalog: &StoredCatalog) -> Self {
        Self {
            id: catalog.id.clone(),
            title: catalog.title.clone(),
            feed_url: catalog.feed_url.clone(),
            auth_kind: catalog.auth.kind(),
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

fn load_catalogs(
    store: &tauri_plugin_store::Store<tauri::Wry>,
) -> Result<Vec<StoredCatalog>, OpdsError> {
    let Some(value) = store.get(CATALOGS_KEY) else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value).map_err(OpdsError::storage)
}

fn save_catalogs(
    store: &tauri_plugin_store::Store<tauri::Wry>,
    catalogs: &[StoredCatalog],
) -> Result<(), OpdsError> {
    let value = serde_json::to_value(catalogs).map_err(OpdsError::storage)?;
    store.set(CATALOGS_KEY, value);
    store.save().map_err(OpdsError::storage)
}

fn find_catalog(
    store: &tauri_plugin_store::Store<tauri::Wry>,
    id: &str,
) -> Result<StoredCatalog, OpdsError> {
    load_catalogs(store)?
        .into_iter()
        .find(|catalog| catalog.id == id)
        .ok_or_else(|| OpdsError::NotFound { id: id.to_string() })
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
fn ensure_secure_auth(url: &Url, auth: &StoredAuth) -> Result<(), OpdsError> {
    if matches!(auth, StoredAuth::None) {
        return Ok(());
    }
    if url.scheme() == "http" && !is_loopback(url) {
        return Err(OpdsError::InsecureCredentials {
            message: format!("refusing to send credentials over plain http to {url}"),
        });
    }
    Ok(())
}

fn client_for(catalog: &StoredCatalog, http: &reqwest::Client) -> Result<Client, OpdsError> {
    let client =
        Client::with_http(catalog.feed_url.clone(), http.clone()).map_err(OpdsError::from)?;
    Ok(client.auth(catalog.auth.to_client_auth()))
}

/// Fetch and flatten a catalog's root feed, proving the URL and credentials work.
async fn probe_feed(
    catalog_url: &Url,
    auth: &StoredAuth,
    http: &reqwest::Client,
) -> Result<OpdsFeed, OpdsError> {
    let client =
        Client::with_http(catalog_url.to_string(), http.clone()).map_err(OpdsError::from)?;
    client
        .auth(auth.to_client_auth())
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
    Ok(load_catalogs(&state.opds_store)?
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
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err(OpdsError::invalid_input("catalog title is required"));
    }
    let url = validate_feed_url(&feed_url)?;
    let auth = StoredAuth::from_input(auth)?;
    ensure_secure_auth(&url, &auth)?;
    probe_feed(&url, &auth, &state.opds_http).await?;

    let mut catalogs = load_catalogs(&state.opds_store)?;
    let now = now_rfc3339();
    let catalog = StoredCatalog {
        id: DbId::new().to_string(),
        title,
        feed_url: url.to_string(),
        auth,
        created_at: now.clone(),
        updated_at: now,
    };
    catalogs.push(catalog.clone());
    save_catalogs(&state.opds_store, &catalogs)?;
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
    let mut catalogs = load_catalogs(&state.opds_store)?;
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
    if let Some(auth_input) = auth {
        catalog.auth = StoredAuth::from_input(auth_input)?;
        needs_probe = true;
    }

    if needs_probe {
        let url = validate_feed_url(&catalog.feed_url)?;
        ensure_secure_auth(&url, &catalog.auth)?;
        probe_feed(&url, &catalog.auth, &state.opds_http).await?;
    }

    catalog.updated_at = now_rfc3339();
    catalogs[index] = catalog.clone();
    save_catalogs(&state.opds_store, &catalogs)?;
    Ok(OpdsCatalog::from(&catalog))
}

#[tauri::command]
#[specta::specta]
pub fn opds_catalogs_remove(id: String, state: State<'_, AppState>) -> Result<(), OpdsError> {
    let mut catalogs = load_catalogs(&state.opds_store)?;
    let before = catalogs.len();
    catalogs.retain(|catalog| catalog.id != id);
    if catalogs.len() == before {
        return Err(OpdsError::NotFound { id });
    }
    save_catalogs(&state.opds_store, &catalogs)
}

#[tauri::command]
#[specta::specta]
pub async fn opds_catalogs_test(
    id: String,
    state: State<'_, AppState>,
) -> Result<OpdsFeed, OpdsError> {
    let catalog = find_catalog(&state.opds_store, &id)?;
    let url = validate_feed_url(&catalog.feed_url)?;
    ensure_secure_auth(&url, &catalog.auth)?;
    probe_feed(&url, &catalog.auth, &state.opds_http).await
}

#[tauri::command]
#[specta::specta]
pub async fn opds_feed(id: String, state: State<'_, AppState>) -> Result<OpdsFeed, OpdsError> {
    let catalog = find_catalog(&state.opds_store, &id)?;
    client_for(&catalog, &state.opds_http)?
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
    if href.trim().is_empty() {
        return Err(OpdsError::invalid_input("page href is required"));
    }
    let catalog = find_catalog(&state.opds_store, &id)?;
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
    client_for(&catalog, &state.opds_http)?
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
    let catalog = find_catalog(&state.opds_store, &id)?;
    let client = client_for(&catalog, &state.opds_http)?;
    livtet_opds_client::search::search(&client, "", &query)
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
    let catalog = find_catalog(&state.opds_store, &id)?;
    let catalog_url = validate_feed_url(&catalog.feed_url)?;
    let url = validate_feed_url(&item_url)?;
    ensure_secure_auth(&url, &catalog.auth)?;

    // Credentials only ever travel to the catalog's own origin; a CDN-hosted
    // acquisition link gets the request without them.
    let mut request = state.opds_http.get(url.clone());
    if same_origin(&catalog_url, &url) {
        request = catalog.auth.to_client_auth().apply(request);
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

    let temp_dir = state.books_dir.join(".opds-tmp");
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

    // The library owns a copy: the temp file is transient and removed below.
    let outcome = import_one(temp_path.as_str(), ImportMode::Copy, state.inner()).await;
    let _ = fs_err::tokio::remove_file(&temp_path).await;
    outcome.map_err(|error| OpdsError::Import {
        code: error.code.clone(),
        message: error.message.clone(),
    })
}

#[cfg(test)]
mod tests {
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
        let basic = StoredAuth::Basic {
            username: "u".into(),
            password: "p".into(),
        };
        let remote = Url::parse("http://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&remote, &basic).is_err());

        let local = Url::parse("http://127.0.0.1:8080/opds").unwrap();
        assert!(ensure_secure_auth(&local, &basic).is_ok());

        let secure = Url::parse("https://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&secure, &basic).is_ok());
    }

    #[test]
    fn no_credentials_never_triggers_insecure_refusal() {
        let remote = Url::parse("http://example.test/opds").unwrap();
        assert!(ensure_secure_auth(&remote, &StoredAuth::None).is_ok());
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
    fn auth_input_requires_the_matching_secret() {
        let missing_username = OpdsAuthInput {
            kind: OpdsAuthKind::Basic,
            username: Some(String::new()),
            password: Some("p".into()),
            token: None,
        };
        assert!(StoredAuth::from_input(missing_username).is_err());

        let missing_token = OpdsAuthInput {
            kind: OpdsAuthKind::Bearer,
            username: None,
            password: None,
            token: None,
        };
        assert!(StoredAuth::from_input(missing_token).is_err());
    }
}
