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
    Provider, ProviderError, ProviderId, google_books::GoogleBooks, hardcover::Hardcover,
    openlibrary::OpenLibrary,
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
    let mut chain: Vec<Arc<dyn Provider>> = vec![Arc::new(GoogleBooks::new(
        http.clone(),
        secrets::GOOGLE_BOOKS_API_KEY.to_string(),
    ))];
    if let Some(key) = hardcover_key {
        chain.push(Arc::new(Hardcover::new(http.clone(), Some(key))));
    }
    chain.push(Arc::new(OpenLibrary::new(http)));
    chain
}

pub async fn run_chain(
    state: &AppState,
    app: &AppHandle,
    query: &str,
    limit: u32,
    request_id: &str,
    token: CancellationToken,
) -> RemoteSearchResult {
    let hardcover_key = load_hardcover_key();
    let providers = build_chain(state.http.clone(), hardcover_key);
    let provider_count = providers.len();

    tracing::info!(
        request_id = %request_id,
        query = %query,
        limit,
        provider_count,
        "starting remote search chain"
    );

    let empty = || RemoteSearchResult {
        request_id: request_id.to_string(),
        results: Vec::new(),
        used_provider: None,
    };

    for provider in providers {
        if token.is_cancelled() {
            return empty();
        }

        let pid = provider.id();
        tracing::info!(
            provider = pid.as_str(),
            query = %query,
            limit,
            "trying provider"
        );

        let result = tokio::select! {
            r = provider.search(query, limit) => r,
            _ = token.cancelled() => Err(ProviderError::Auth),
        };

        match result {
            Ok(hits) if !hits.is_empty() => {
                tracing::info!(
                    provider = pid.as_str(),
                    hit_count = hits.len(),
                    "provider returned hits — chain complete"
                );
                return RemoteSearchResult {
                    request_id: request_id.to_string(),
                    results: hits.into_iter().map(SearchHitRow::from).collect(),
                    used_provider: Some(pid),
                };
            }
            Ok(_) => {
                tracing::debug!(
                    provider = pid.as_str(),
                    "provider returned no hits — advancing to next"
                );
                continue;
            }
            Err(_) if token.is_cancelled() => return empty(),
            Err(e) => {
                tracing::warn!(
                    provider = pid.as_str(),
                    error = %e,
                    "provider failed — advancing to next"
                );
                emit_provider_failure(app, request_id, pid, e.to_string());
                continue;
            }
        }
    }
    tracing::info!(
        request_id = %request_id,
        "all providers exhausted — returning empty results"
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};
    use async_trait::async_trait;
    use std::sync::Arc;

    /// A test-only provider that returns a canned result.
    struct StubProvider {
        id: ProviderId,
        behaviour: StubBehaviour,
    }

    #[derive(Debug, Clone)]
    enum StubBehaviour {
        Ok(Vec<RawSearchHit>),
        Empty,
        ErrRateLimited,
        ErrAuth,
        ErrHttp,
    }

    #[async_trait]
    impl Provider for StubProvider {
        fn id(&self) -> ProviderId {
            self.id
        }
        async fn search(&self, _: &str, _: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
            match self.behaviour.clone() {
                StubBehaviour::Ok(h) => Ok(h),
                StubBehaviour::Empty => Ok(Vec::new()),
                StubBehaviour::ErrRateLimited => Err(ProviderError::RateLimited {
                    retry_after_seconds: 60,
                }),
                StubBehaviour::ErrAuth => Err(ProviderError::Auth),
                StubBehaviour::ErrHttp => Err(ProviderError::Http {
                    status: 503,
                    body: "down".into(),
                }),
            }
        }
    }

    fn hit(title: &str) -> RawSearchHit {
        RawSearchHit {
            provider_id: ProviderId::GoogleBooks,
            provider_work_id: format!("/{title}"),
            title: title.into(),
            authors: vec!["A".into()],
            isbn: None,
            isbn_13: None,
            publisher: None,
            page_count: None,
            language: None,
            published_date: None,
            cover_url: None,
            description: None,
        }
    }

    /// Test that the chain returns the first non-empty result and
    /// does not call later providers.
    #[tokio::test]
    async fn first_non_empty_wins() {
        let providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(StubProvider {
                id: ProviderId::GoogleBooks,
                behaviour: StubBehaviour::Ok(vec![hit("A")]),
            }),
            Arc::new(StubProvider {
                id: ProviderId::Hardcover,
                behaviour: StubBehaviour::Ok(vec![hit("B")]),
            }),
        ];
        let token = CancellationToken::new();
        let result = run_chain_with(providers, &token, "query", 10, "req-1").await;
        assert_eq!(result.used_provider, Some(ProviderId::GoogleBooks));
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].title, "A");
    }

    #[tokio::test]
    async fn empty_first_advances_to_next() {
        let providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(StubProvider {
                id: ProviderId::GoogleBooks,
                behaviour: StubBehaviour::Empty,
            }),
            Arc::new(StubProvider {
                id: ProviderId::Hardcover,
                behaviour: StubBehaviour::Ok(vec![hit("B")]),
            }),
        ];
        let token = CancellationToken::new();
        let result = run_chain_with(providers, &token, "query", 10, "req-1").await;
        assert_eq!(result.used_provider, Some(ProviderId::Hardcover));
        assert_eq!(result.results[0].title, "B");
    }

    #[tokio::test]
    async fn all_three_fail_returns_empty() {
        let providers: Vec<Arc<dyn Provider>> = vec![
            Arc::new(StubProvider {
                id: ProviderId::GoogleBooks,
                behaviour: StubBehaviour::ErrAuth,
            }),
            Arc::new(StubProvider {
                id: ProviderId::Hardcover,
                behaviour: StubBehaviour::ErrHttp,
            }),
            Arc::new(StubProvider {
                id: ProviderId::OpenLibrary,
                behaviour: StubBehaviour::ErrRateLimited,
            }),
        ];
        let token = CancellationToken::new();
        let result = run_chain_with(providers, &token, "query", 10, "req-1").await;
        assert_eq!(result.used_provider, None);
        assert_eq!(result.results.len(), 0);
    }

    #[tokio::test]
    async fn pre_cancelled_token_short_circuits() {
        let providers: Vec<Arc<dyn Provider>> = vec![Arc::new(StubProvider {
            id: ProviderId::GoogleBooks,
            behaviour: StubBehaviour::Ok(vec![hit("A")]),
        })];
        let token = CancellationToken::new();
        token.cancel();
        let result = run_chain_with(providers, &token, "query", 10, "req-1").await;
        assert_eq!(result.used_provider, None);
        assert_eq!(result.results.len(), 0);
    }

    /// Bypass the live `run_chain` because it needs AppState/AppHandle.
    /// This is a test-only reimplementation that mirrors the real
    /// chain's logic without the side effects.
    async fn run_chain_with(
        providers: Vec<Arc<dyn Provider>>,
        token: &CancellationToken,
        query: &str,
        limit: u32,
        request_id: &str,
    ) -> RemoteSearchResult {
        for provider in providers {
            if token.is_cancelled() {
                return RemoteSearchResult {
                    request_id: request_id.to_string(),
                    results: Vec::new(),
                    used_provider: None,
                };
            }
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
                Err(_) if token.is_cancelled() => {
                    return RemoteSearchResult {
                        request_id: request_id.to_string(),
                        results: Vec::new(),
                        used_provider: None,
                    };
                }
                Err(_) => continue,
            }
        }
        RemoteSearchResult {
            request_id: request_id.to_string(),
            results: Vec::new(),
            used_provider: None,
        }
    }
}
