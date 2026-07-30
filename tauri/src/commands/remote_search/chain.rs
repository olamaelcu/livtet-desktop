//! Chain orchestration and the per-search cancellation registry.
//!
//! `SearchRegistry` is a single-slot holder: at most one search is
//! considered "active" at a time. The frontend never has two
//! concurrent searches (debounced input), so a single slot is enough.
//! A new `begin()` call auto-cancels whichever search was previously
//! in the slot; `cancel(id)` is the explicit cancel path.

use std::sync::Arc;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::commands::remote_search::{
    google_books::GoogleBooks, hardcover::Hardcover, openlibrary::OpenLibrary,
    Provider, ProviderError, ProviderId,
};
use crate::commands::search::SearchHitRow;
use crate::secrets;
use crate::state::AppState;

pub const PROVIDER_FAILURE_EVENT: &str = "provider-failure";

#[derive(Debug, Clone, Serialize, Type)]
pub struct ProviderFailureEvent {
    pub request_id: String,
    pub provider: ProviderId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct RemoteSearchResult {
    pub request_id: String,
    pub results: Vec<SearchHitRow>,
    pub used_provider: Option<ProviderId>,
}

#[derive(Default)]
pub struct SearchRegistry {
    active: Mutex<Option<(String, CancellationToken)>>,
}

impl SearchRegistry {
    pub async fn begin(&self, request_id: String) -> CancellationToken {
        let token = CancellationToken::new();
        let mut slot = self.active.lock().await;
        if let Some((prev_id, prev_token)) = slot.take() {
            debug!(request_id = %prev_id, "auto-cancelling previous search");
            prev_token.cancel();
        }
        *slot = Some((request_id, token.clone()));
        token
    }

    pub async fn cancel(&self, request_id: &str) -> bool {
        let slot = self.active.lock().await;
        match slot.as_ref() {
            Some((id, token)) if id == request_id => {
                token.cancel();
                true
            }
            _ => false,
        }
    }

    pub async fn finish(&self) {
        self.active.lock().await.take();
    }
}

fn load_hardcover_key() -> Option<String> {
    use keyring::Entry;
    let entry = match Entry::new(KEYRING_SERVICE, KEYRING_USER) {
        Ok(e) => e,
        Err(e) => {
            warn!(error = %e, "keyring unavailable; treating Hardcover as unconfigured");
            return None;
        }
    };
    match entry.get_password() {
        Ok(k) => Some(k),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            warn!(error = %e, "keyring read failed; treating Hardcover as unconfigured");
            None
        }
    }
}

const KEYRING_SERVICE: &str = "net.olamaelcu.livtet";
const KEYRING_USER: &str = "hardcover_api_key";

pub fn build_chain(http: reqwest::Client, hardcover_key: Option<String>) -> Vec<Arc<dyn Provider>> {
    let mut chain: Vec<Arc<dyn Provider>> = vec![
        Arc::new(GoogleBooks::new(http.clone(), secrets::GOOGLE_BOOKS_API_KEY.to_string())),
    ];
    if let Some(key) = hardcover_key {
        chain.push(Arc::new(Hardcover::new(http.clone(), Some(key))));
    }
    chain.push(Arc::new(OpenLibrary::new(http)));
    chain
}

pub async fn run_chain(
    _state: &AppState,
    app: &AppHandle,
    query: &str,
    limit: u32,
    request_id: &str,
    token: CancellationToken,
) -> RemoteSearchResult {
    let hardcover_key = load_hardcover_key();
    let providers = build_chain(_state.http.clone(), hardcover_key);

    let empty = || RemoteSearchResult {
        request_id: request_id.to_string(),
        results: Vec::new(),
        used_provider: None,
    };

    for provider in providers {
        if token.is_cancelled() { return empty(); }

        let result = tokio::select! {
            r = provider.search(query, limit) => r,
            _ = token.cancelled() => Err(ProviderError::Auth),
        };

        match result {
            Ok(hits) if !hits.is_empty() => {
                return RemoteSearchResult {
                    request_id: request_id.to_string(),
                    results: hits.into_iter().map(SearchHitRow::from).collect(),
                    used_provider: Some(provider.id()),
                };
            }
            Ok(_) => continue,
            Err(_) if token.is_cancelled() => return empty(),
            Err(e) => {
                emit_provider_failure(app, request_id, provider.id(), e.to_string());
                continue;
            }
        }
    }
    empty()
}

fn emit_provider_failure(app: &AppHandle, request_id: &str, provider: ProviderId, reason: String) {
    if let Err(e) = app.emit(
        PROVIDER_FAILURE_EVENT,
        ProviderFailureEvent {
            request_id: request_id.to_string(),
            provider,
            reason,
        },
    ) {
        warn!(error = %e, "failed to emit provider-failure event");
    }
}
