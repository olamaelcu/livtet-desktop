//! Shared HTTP client construction.
//!
//! Single source of truth for the User-Agent and the default request
//! timeout. Providers and live integration tests should build their
//! client via [`build_client`] and rely on its defaults instead of
//! re-declaring them per request.

use std::time::Duration;

// FIXME: Derive the version from the crate itself.
pub const USER_AGENT: &str = "livtet-desktop/0.1.0 (+https://livtet.olamaelcu.net/apps)";

/// Build the shared `reqwest::Client`. Default User-Agent is
/// [`USER_AGENT`]; default per-request timeout is 8s.
pub fn build_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(8))
        .build()
        .expect("failed to build reqwest client")
}

