//! OpenLibrary provider (stub — implemented in Task 9).
use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};

pub struct OpenLibrary {
    #[allow(dead_code)]
    http: reqwest::Client,
}

impl OpenLibrary {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

#[async_trait::async_trait]
impl Provider for OpenLibrary {
    fn id(&self) -> ProviderId { ProviderId::OpenLibrary }
    async fn search(&self, _query: &str, _limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        unimplemented!("Task 9")
    }
}
