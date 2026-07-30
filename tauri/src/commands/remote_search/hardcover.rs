//! Hardcover provider (stub — implemented in Task 8).
use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};

pub struct Hardcover {
    #[allow(dead_code)]
    http: reqwest::Client,
    #[allow(dead_code)]
    api_key: Option<String>,
}

impl Hardcover {
    pub fn new(http: reqwest::Client, api_key: Option<String>) -> Self {
        Self { http, api_key }
    }
}

#[async_trait::async_trait]
impl Provider for Hardcover {
    fn id(&self) -> ProviderId { ProviderId::Hardcover }
    async fn search(&self, _query: &str, _limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        unimplemented!("Task 8")
    }
}
