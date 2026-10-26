pub(crate) mod client_metadata;
pub(crate) mod http_log;
pub(crate) mod loopback;
pub(crate) mod manager;
pub(crate) mod shelf_records;
pub(crate) mod store;
pub mod types;

pub(crate) use manager::AtprotoManager;
pub use types::{AtprotoLoginStart, AtprotoProfileView, AtprotoStatus};

pub(crate) const CLIENT_ORIGIN: &str = "https://livtet.olamaelcu.net";
pub(crate) const BOOTSTRAP_METADATA_URL: &str =
    "https://livtet.olamaelcu.net/.well-known/oauth-client-metadata.json";
pub(crate) const REDIRECT_URI: &str = "net.olamaelcu.livtet:/oauth-redirect";
/// Loopback redirect advertised by the native client document; used instead of
/// the custom scheme when the server lists it, so no OS scheme registration is
/// needed.
pub(crate) const LOOPBACK_REDIRECT_URI: &str = "http://127.0.0.1:47821/oauth-redirect";
pub(crate) const APPVIEW_URL: &str = "https://biblio.livtet.olamaelcu.net";
pub(crate) const APPVIEW_PROFILE_NSID: &str = "net.olamaelcu.livtet.biblio.getActorProfile";
/// The user's Livtet profile record on their own PDS.
pub(crate) const PROFILE_COLLECTION: &str = "net.olamaelcu.livtet.actor.profile";
pub(crate) const PROFILE_RKEY: &str = "self";
/// The Livtet platform serves the account's Bluesky avatar when the Livtet
/// record has none.
pub(crate) const PLATFORM_AVATAR_BASE: &str = "https://livtet.olamaelcu.net/user/profileImage";
/// PDS blob limit for an avatar, matching the Bluesky client.
pub(crate) const MAX_AVATAR_BYTES: usize = 1_000_000;
pub(crate) const DOH_URL: &str = "https://cloudflare-dns.com/dns-query";
