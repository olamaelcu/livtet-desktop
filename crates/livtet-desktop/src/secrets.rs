//! OS-keyring-backed storage for OPDS catalog credentials.
//!
//! Secrets live in the platform credential store — Secret Service on Linux,
//! Keychain on macOS, Credential Manager on Windows — via the `keyring` crate.
//! The Rust-owned catalog store (`opds-catalogs.json`) holds only metadata and
//! the auth kind; it never carries a secret.
//!
//! [`SecretStore`] keeps the blocking keyring behind a seam so command code can
//! run it on a blocking thread and tests can swap in an in-memory store. There
//! is deliberately no plaintext fallback: when the OS keyring is unavailable,
//! [`SecretStore`] calls fail and callers fail closed.

#[cfg(test)]
use std::collections::HashMap;
#[cfg(test)]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Service name under which OPDS credentials are registered in the OS keyring.
pub const KEYRING_SERVICE: &str = "net.olamaelcu.livtet.opds";

/// The secret half of a catalog's credentials, keyed by the catalog ULID.
///
/// Serialized to JSON as the keyring value; never written to the app data dir.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CatalogSecret {
    Basic { username: String, password: String },
    Bearer { token: String },
}

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The OS credential store could not be reached (no Secret Service, locked
    /// keychain, missing D-Bus session, ...). Callers must fail closed.
    #[error("OS keyring is unavailable: {0}")]
    Unavailable(String),

    /// A value was found but could not be decoded as a [`CatalogSecret`].
    #[error("stored credential is malformed: {0}")]
    Malformed(String),
}

/// Blocking access to the OS credential store, keyed by catalog id.
pub trait SecretStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<CatalogSecret>, SecretError>;
    fn set(&self, id: &str, secret: &CatalogSecret) -> Result<(), SecretError>;
    fn delete(&self, id: &str) -> Result<(), SecretError>;
}

/// Real store backed by the platform keyring, chosen by the `keyring` crate
/// (Secret Service / Keychain / Credential Manager).
pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, id: &str) -> Result<Option<CatalogSecret>, SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(unavailable)?;
        match entry.get_password() {
            Ok(json) => serde_json::from_str(&json)
                .map(Some)
                .map_err(|error| SecretError::Malformed(error.to_string())),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(unavailable(error)),
        }
    }

    fn set(&self, id: &str, secret: &CatalogSecret) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(unavailable)?;
        let json = serde_json::to_string(secret)
            .map_err(|error| SecretError::Malformed(error.to_string()))?;
        entry.set_password(&json).map_err(unavailable)
    }

    fn delete(&self, id: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, id).map_err(unavailable)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(unavailable(error)),
        }
    }
}

fn unavailable<E: std::fmt::Display>(error: E) -> SecretError {
    SecretError::Unavailable(error.to_string())
}

/// In-memory store for tests.
#[cfg(test)]
#[derive(Default)]
pub struct InMemorySecretStore {
    entries: Mutex<HashMap<String, CatalogSecret>>,
}

#[cfg(test)]
impl InMemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
impl SecretStore for InMemorySecretStore {
    fn get(&self, id: &str) -> Result<Option<CatalogSecret>, SecretError> {
        Ok(self
            .entries
            .lock()
            .expect("secret store lock")
            .get(id)
            .cloned())
    }

    fn set(&self, id: &str, secret: &CatalogSecret) -> Result<(), SecretError> {
        self.entries
            .lock()
            .expect("secret store lock")
            .insert(id.to_string(), secret.clone());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), SecretError> {
        self.entries.lock().expect("secret store lock").remove(id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_round_trips_secrets() {
        let store = InMemorySecretStore::new();
        let secret = CatalogSecret::Basic {
            username: "patron@example.test".into(),
            password: String::new(),
        };

        assert_eq!(store.get("01ABC").unwrap(), None);
        store.set("01ABC", &secret).unwrap();
        assert_eq!(store.get("01ABC").unwrap(), Some(secret));
        store.delete("01ABC").unwrap();
        assert_eq!(store.get("01ABC").unwrap(), None);
        // Deleting a missing entry is not an error.
        store.delete("01ABC").unwrap();
    }

    #[test]
    fn catalog_secret_serializes_with_a_kind_tag() {
        let bearer = CatalogSecret::Bearer { token: "t".into() };
        let json = serde_json::to_string(&bearer).unwrap();
        assert_eq!(json, r#"{"kind":"bearer","token":"t"}"#);
    }
}
