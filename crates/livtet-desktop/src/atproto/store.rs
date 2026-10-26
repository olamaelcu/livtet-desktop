//! Keyring-backed OAuth state and session stores.
//!
//! Each store owns one keyring entry holding a JSON object: sessions keyed by
//! DID, state keyed by the OAuth `state` value. Keyring I/O is blocking, so
//! every operation runs on the blocking pool and a per-store mutex serialises
//! the read-modify-write cycle.

use std::collections::BTreeMap;
use std::sync::Arc;

use atrium_api::types::string::Did;
use atrium_common::store::Store;
use atrium_oauth::store::session::Session;
use atrium_oauth::store::state::InternalStateData;
use keyring::Entry;
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

/// Keyring service name shared by every ATProto entry.
pub(crate) const KEYRING_SERVICE: &str = "net.olamaelcu.livtet.atproto";
const SESSIONS_ACCOUNT: &str = "sessions";
const STATE_ACCOUNT: &str = "state";
const CURRENT_ACCOUNT: &str = "current";

/// Failure from the keyring or from serialising a store.
#[derive(Debug, Clone, Error)]
#[error("ATProto store error: {message}")]
pub(crate) struct AtprotoStoreError {
    pub message: String,
}

impl AtprotoStoreError {
    fn new(error: impl std::fmt::Display) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<keyring::Error> for AtprotoStoreError {
    fn from(error: keyring::Error) -> Self {
        Self::new(error)
    }
}

/// Synchronous keyring seam so tests never touch the OS keyring.
pub(crate) trait KeyValueBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError>;
    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError>;
    fn del(&self, account: &str) -> Result<(), AtprotoStoreError>;
}

/// The OS keyring.
pub(crate) struct KeyringBackend;

impl KeyValueBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        match Entry::new(KEYRING_SERVICE, account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AtprotoStoreError::new(error)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        Entry::new(KEYRING_SERVICE, account)?.set_password(value)?;
        Ok(())
    }

    fn del(&self, account: &str) -> Result<(), AtprotoStoreError> {
        match Entry::new(KEYRING_SERVICE, account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AtprotoStoreError::new(error)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AtprotoStateStore {
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
}

#[derive(Clone)]
pub(crate) struct AtprotoSessionStore {
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl AtprotoStateStore {
    pub(crate) fn new(backend: Arc<dyn KeyValueBackend>) -> Self {
        Self {
            backend,
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

impl AtprotoSessionStore {
    pub(crate) fn new(backend: Arc<dyn KeyValueBackend>) -> Self {
        Self {
            backend,
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }
}

/// Read the account's JSON map, letting `mutate` change it, then write it back.
async fn update_map<V, F>(
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
    account: &'static str,
    mutate: F,
) -> Result<(), AtprotoStoreError>
where
    V: DeserializeOwned + Serialize + Send + 'static,
    F: FnOnce(&mut BTreeMap<String, V>) + Send + 'static,
{
    let _guard = lock.lock().await;
    tokio::task::spawn_blocking(move || {
        let mut map = read_map::<V>(&*backend, account)?;
        mutate(&mut map);
        let raw = serde_json::to_string(&map).map_err(AtprotoStoreError::new)?;
        backend.set(account, &raw)
    })
    .await
    .map_err(AtprotoStoreError::new)?
}

/// Read one entry out of the account's JSON map.
async fn read_entry<V>(
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
    account: &'static str,
    key: String,
) -> Result<Option<V>, AtprotoStoreError>
where
    V: DeserializeOwned + Clone + Send + 'static,
{
    let _guard = lock.lock().await;
    tokio::task::spawn_blocking(move || Ok(read_map::<V>(&*backend, account)?.get(&key).cloned()))
        .await
        .map_err(AtprotoStoreError::new)?
}

fn read_map<V: DeserializeOwned>(
    backend: &dyn KeyValueBackend,
    account: &str,
) -> Result<BTreeMap<String, V>, AtprotoStoreError> {
    match backend.get(account)? {
        Some(raw) => serde_json::from_str(&raw).map_err(AtprotoStoreError::new),
        None => Ok(BTreeMap::new()),
    }
}

impl Store<String, InternalStateData> for AtprotoStateStore {
    type Error = AtprotoStoreError;

    async fn get(&self, key: &String) -> Result<Option<InternalStateData>, Self::Error> {
        read_entry(
            self.backend.clone(),
            self.lock.clone(),
            STATE_ACCOUNT,
            key.clone(),
        )
        .await
    }

    async fn set(&self, key: String, value: InternalStateData) -> Result<(), Self::Error> {
        update_map::<InternalStateData, _>(
            self.backend.clone(),
            self.lock.clone(),
            STATE_ACCOUNT,
            move |map| {
                map.insert(key, value);
            },
        )
        .await
    }

    async fn del(&self, key: &String) -> Result<(), Self::Error> {
        let key = key.clone();
        update_map::<InternalStateData, _>(
            self.backend.clone(),
            self.lock.clone(),
            STATE_ACCOUNT,
            move |map| {
                map.remove(&key);
            },
        )
        .await
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        let _guard = self.lock.lock().await;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || backend.del(STATE_ACCOUNT))
            .await
            .map_err(AtprotoStoreError::new)?
    }
}

impl Store<Did, Session> for AtprotoSessionStore {
    type Error = AtprotoStoreError;

    async fn get(&self, key: &Did) -> Result<Option<Session>, Self::Error> {
        read_entry(
            self.backend.clone(),
            self.lock.clone(),
            SESSIONS_ACCOUNT,
            key.to_string(),
        )
        .await
    }

    async fn set(&self, key: Did, value: Session) -> Result<(), Self::Error> {
        update_map::<Session, _>(
            self.backend.clone(),
            self.lock.clone(),
            SESSIONS_ACCOUNT,
            move |map| {
                map.insert(key.to_string(), value);
            },
        )
        .await
    }

    async fn del(&self, key: &Did) -> Result<(), Self::Error> {
        let key = key.to_string();
        update_map::<Session, _>(
            self.backend.clone(),
            self.lock.clone(),
            SESSIONS_ACCOUNT,
            move |map| {
                map.remove(&key);
            },
        )
        .await
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        let _guard = self.lock.lock().await;
        let backend = self.backend.clone();
        tokio::task::spawn_blocking(move || backend.del(SESSIONS_ACCOUNT))
            .await
            .map_err(AtprotoStoreError::new)?
    }
}

impl atrium_oauth::store::state::StateStore for AtprotoStateStore {}
impl atrium_oauth::store::session::SessionStore for AtprotoSessionStore {}

/// The DID of the signed-in account, if any.
pub(crate) async fn get_current_did(
    backend: Arc<dyn KeyValueBackend>,
) -> Result<Option<String>, AtprotoStoreError> {
    tokio::task::spawn_blocking(move || backend.get(CURRENT_ACCOUNT))
        .await
        .map_err(AtprotoStoreError::new)?
}

/// Remember the signed-in account.
pub(crate) async fn set_current_did(
    backend: Arc<dyn KeyValueBackend>,
    did: &str,
) -> Result<(), AtprotoStoreError> {
    let did = did.to_string();
    tokio::task::spawn_blocking(move || backend.set(CURRENT_ACCOUNT, &did))
        .await
        .map_err(AtprotoStoreError::new)?
}

/// Forget the signed-in account.
pub(crate) async fn clear_current_did(
    backend: Arc<dyn KeyValueBackend>,
) -> Result<(), AtprotoStoreError> {
    tokio::task::spawn_blocking(move || backend.del(CURRENT_ACCOUNT))
        .await
        .map_err(AtprotoStoreError::new)?
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) struct MemoryBackend {
    store: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
impl MemoryBackend {
    pub(crate) fn new() -> Self {
        Self {
            store: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
impl KeyValueBackend for MemoryBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        self.store
            .lock()
            .unwrap()
            .insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn del(&self, account: &str) -> Result<(), AtprotoStoreError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    const DID_A: &str = "did:plc:aaaaaaaaaaaaaaaaaaaaaaaa";
    const DID_B: &str = "did:plc:bbbbbbbbbbbbbbbbbbbbbbbb";

    /// A syntactically valid P-256 JWK; the coordinates need not be on-curve
    /// for a serde round trip.
    fn jwk() -> serde_json::Value {
        let coord = "A".repeat(43);
        serde_json::json!({
            "kty": "EC",
            "crv": "P-256",
            "x": coord,
            "y": coord,
            "d": coord,
        })
    }

    fn state_fixture(verifier: &str) -> InternalStateData {
        serde_json::from_value(serde_json::json!({
            "iss": "https://pds.example",
            "dpop_key": jwk(),
            "verifier": verifier,
            "redirect_uri": "net.olamaelcu.livtet:/oauth-redirect",
            "app_state": null,
        }))
        .expect("state fixture")
    }

    fn session_fixture(sub: &str) -> Session {
        serde_json::from_value(serde_json::json!({
            "dpop_key": jwk(),
            "token_set": {
                "iss": "https://pds.example",
                "sub": sub,
                "aud": "https://pds.example",
                "scope": "atproto",
                "refresh_token": "refresh-token",
                "access_token": "access-token",
                "token_type": "DPoP",
                "expires_at": null,
            },
        }))
        .expect("session fixture")
    }

    fn backend() -> Arc<MemoryBackend> {
        Arc::new(MemoryBackend::new())
    }

    fn did(value: &str) -> Did {
        value.parse().expect("did")
    }

    #[tokio::test]
    async fn state_store_round_trips_internal_state_data() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let store = AtprotoStateStore::new(backend);
        let key = "state-key".to_string();

        assert!(store.get(&key).await.unwrap().is_none());

        store
            .set(key.clone(), state_fixture("verifier-one"))
            .await
            .unwrap();
        let stored = store.get(&key).await.unwrap().expect("state present");
        assert_eq!(stored.verifier, "verifier-one");
        assert_eq!(stored.iss, "https://pds.example");

        // A second set keeps the first key.
        store
            .set("other-key".to_string(), state_fixture("verifier-two"))
            .await
            .unwrap();
        assert!(store.get(&key).await.unwrap().is_some());
        assert!(store.get(&"other-key".to_string()).await.unwrap().is_some());

        store.del(&key).await.unwrap();
        assert!(store.get(&key).await.unwrap().is_none());
        assert!(store.get(&"other-key".to_string()).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn session_store_round_trips_a_session() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let store = AtprotoSessionStore::new(backend);
        let key = did(DID_A);

        store
            .set(key.clone(), session_fixture(DID_A))
            .await
            .unwrap();

        let stored = store.get(&key).await.unwrap().expect("session present");
        assert_eq!(stored.token_set.sub, key);
        assert_eq!(stored.token_set.access_token, "access-token");
        assert_eq!(stored.token_set.scope.as_deref(), Some("atproto"));

        store.del(&key).await.unwrap();
        assert!(store.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn session_store_two_dids_coexist() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let store = AtprotoSessionStore::new(backend);

        store.set(did(DID_A), session_fixture(DID_A)).await.unwrap();
        store.set(did(DID_B), session_fixture(DID_B)).await.unwrap();

        assert_eq!(
            store.get(&did(DID_A)).await.unwrap().unwrap().token_set.sub,
            did(DID_A)
        );
        assert_eq!(
            store.get(&did(DID_B)).await.unwrap().unwrap().token_set.sub,
            did(DID_B)
        );

        store.del(&did(DID_A)).await.unwrap();
        assert!(store.get(&did(DID_A)).await.unwrap().is_none());
        assert!(store.get(&did(DID_B)).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn clear_removes_every_session() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let store = AtprotoSessionStore::new(backend);

        store.set(did(DID_A), session_fixture(DID_A)).await.unwrap();
        store.set(did(DID_B), session_fixture(DID_B)).await.unwrap();
        store.clear().await.unwrap();

        assert!(store.get(&did(DID_A)).await.unwrap().is_none());
        assert!(store.get(&did(DID_B)).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn clear_removes_every_state() {
        let backend: Arc<dyn KeyValueBackend> = backend();
        let store = AtprotoStateStore::new(backend);

        store
            .set("first".to_string(), state_fixture("one"))
            .await
            .unwrap();
        store
            .set("second".to_string(), state_fixture("two"))
            .await
            .unwrap();
        store.clear().await.unwrap();

        assert!(store.get(&"first".to_string()).await.unwrap().is_none());
        assert!(store.get(&"second".to_string()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn current_did_round_trips() {
        let backend: Arc<dyn KeyValueBackend> = backend();

        assert!(get_current_did(backend.clone()).await.unwrap().is_none());

        set_current_did(backend.clone(), DID_A).await.unwrap();
        assert_eq!(
            get_current_did(backend.clone()).await.unwrap().as_deref(),
            Some(DID_A)
        );

        clear_current_did(backend.clone()).await.unwrap();
        assert!(get_current_did(backend).await.unwrap().is_none());
    }
}
