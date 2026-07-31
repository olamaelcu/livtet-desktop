//! Online book search: provider trait, raw hit shape, error model.
//!
//! The chain that orchestrates providers lives in `chain.rs`. The
//! three concrete providers live in `google_books.rs`, `hardcover.rs`,
//! and `openlibrary.rs`. They each implement the `Provider` trait
//! below and convert their typed response into `Vec<RawSearchHit>`.

use async_trait::async_trait;
use miette::Diagnostic;
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

use crate::commands::search::SearchHitRow;

pub mod chain;
pub mod google_books;
pub mod hardcover;
pub mod openlibrary;

/// Identifies which provider answered a search hit. Serialised
/// snake_case so the specta-generated TS type is narrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    GoogleBooks,
    Hardcover,
    OpenLibrary,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderId::GoogleBooks => "google_books",
            ProviderId::Hardcover => "hardcover",
            ProviderId::OpenLibrary => "openlibrary",
        }
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> ProviderId;

    /// The maximum `limit` this provider will accept. The chain
    /// clamps the caller's requested limit to this value before
    /// invoking [`search`][Self::search]. Defaults to `u32::MAX`
    /// (no cap); providers backed by an API with a page-size ceiling
    /// should override.
    fn max_limit(&self) -> u32 {
        u32::MAX
    }

    async fn search(&self, query: &str, limit: u32) -> Result<Vec<RawSearchHit>, ProviderError>;
}

/// Provider-agnostic book hit. The chain converts each into a
/// `SearchHitRow` before returning to the webview.
#[derive(Debug, Clone)]
pub struct RawSearchHit {
    pub provider_id: ProviderId,
    pub provider_work_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub isbn: Option<String>,
    pub isbn_13: Option<String>,
    pub publisher: Option<String>,
    pub page_count: Option<u32>,
    pub language: Option<String>,
    pub published_date: Option<String>,
    pub cover_url: Option<String>,
    pub description: Option<String>,
}

impl From<RawSearchHit> for SearchHitRow {
    fn from(r: RawSearchHit) -> Self {
        Self {
            kind: livtet_core::search::HitKind::Work,
            edition_id: None,
            work_id: r.provider_work_id,
            author_id: None,
            title: r.title.clone(),
            work_title: None,
            edition_title: None,
            authors: r.authors,
            isbn: r.isbn,
            format: None,
            language: r.language,
            published_date: r.published_date,
            score: 0.0,
            explanation: None,
            snippet_text: None,
            snippet_highlighted: Vec::new(),
            grouped_edition_ids: Vec::new(),
            source: r.provider_id.as_str().to_string(),
            publisher: r.publisher,
            page_count: r.page_count,
            cover_url: r.cover_url,
            description: r.description,
            isbn_13: r.isbn_13,
        }
    }
}

#[derive(Debug, Error, Diagnostic)]
pub enum ProviderError {
    #[error("HTTP {status}: {body}")]
    #[diagnostic(code(provider::http))]
    Http { status: u16, body: String },

    #[error("rate-limited (retry after {retry_after_seconds}s)")]
    #[diagnostic(code(provider::rate_limited))]
    RateLimited { retry_after_seconds: u32 },

    #[error("missing or invalid API key")]
    #[diagnostic(
        code(provider::auth),
        help("Add this provider's API key on the Settings page to enable it.")
    )]
    Auth,

    #[error("malformed response: {0}")]
    #[diagnostic(code(provider::parse))]
    Parse(String),

    #[error("GraphQL error: {messages:?}")]
    #[diagnostic(code(provider::graphql))]
    GraphQL { messages: Vec<String> },

    #[error("transport: {0}")]
    #[diagnostic(code(provider::transport))]
    Transport(#[from] reqwest::Error),

    #[error("timeout")]
    #[diagnostic(code(provider::timeout))]
    Timeout,
}

use tauri::{AppHandle, State};

use crate::commands::remote_search::chain::{RemoteSearchResult, run_chain};
use crate::state::AppState;

#[tauri::command]
#[specta::specta]
pub async fn remote_search(
    state: State<'_, AppState>,
    app: AppHandle,
    query: String,
    limit: u32,
    request_id: String,
) -> Result<RemoteSearchResult, String> {
    let token = state.search_registry.begin(request_id.clone()).await;
    let result = run_chain(&state, &app, &query, limit, &request_id, token).await;
    state.search_registry.finish().await;
    Ok(result)
}

#[tauri::command]
#[specta::specta]
pub async fn cancel_remote_search(
    state: State<'_, AppState>,
    request_id: String,
) -> Result<bool, String> {
    Ok(state.search_registry.cancel(&request_id).await)
}
