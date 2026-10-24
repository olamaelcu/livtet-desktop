pub(crate) mod client_metadata;
pub(crate) mod manager;
pub(crate) mod store;
pub mod types;

pub(crate) use manager::AtprotoManager;
pub use types::{AtprotoLoginStart, AtprotoProfileView, AtprotoStatus};

pub(crate) const CLIENT_ORIGIN: &str = "https://livtet.olamaelcu.net";
pub(crate) const BOOTSTRAP_METADATA_URL: &str =
    "https://livtet.olamaelcu.net/.well-known/oauth-client-metadata.json";
pub(crate) const REDIRECT_URI: &str = "net.olamaelcu.livtet:/oauth-redirect";
pub(crate) const APPVIEW_URL: &str = "https://biblio.livtet.olamaelcu.net";
pub(crate) const APPVIEW_PROFILE_NSID: &str = "net.olamaelcu.livtet.biblio.getActorProfile";
pub(crate) const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";
