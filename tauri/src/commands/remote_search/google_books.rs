//! Google Books provider (stub — implemented in Task 7).
use crate::commands::remote_search::{Provider, ProviderError, ProviderId, RawSearchHit};

pub struct GoogleBooks {
    #[allow(dead_code)]
    http: reqwest::Client,
    #[allow(dead_code)]
    api_key: String,
}

impl GoogleBooks {
    pub fn new(http: reqwest::Client, api_key: String) -> Self {
        Self { http, api_key }
    }
}

#[async_trait::async_trait]
impl Provider for GoogleBooks {
    fn id(&self) -> ProviderId { ProviderId::GoogleBooks }
    async fn search(&self, _query: &str, _limit: u32) -> Result<Vec<RawSearchHit>, ProviderError> {
        unimplemented!("Task 7")
    }
}
