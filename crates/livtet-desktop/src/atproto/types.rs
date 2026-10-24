use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
pub struct AtprotoLoginStart {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AtprotoStatus {
    pub did: String,
    pub scope: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
pub struct AtprotoProfileView {
    pub did: String,
    pub handle: Option<String>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub avatar_cid: Option<String>,
    /// Resolved image URL: the Livtet avatar blob when set, otherwise the
    /// account's Bluesky avatar through the Livtet platform's proxy.
    pub avatar_url: Option<String>,
}
