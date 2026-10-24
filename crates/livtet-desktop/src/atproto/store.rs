use std::collections::BTreeMap;
use std::sync::Arc;
use keyring::Entry;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub(crate) struct AtprotoStoreError {
    pub message: String,
}

impl std::fmt::Display for AtprotoStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ATProto store error: {}", self.message)
    }
}

impl From<keyring::Error> for AtprotoStoreError {
    fn from(e: keyring::Error) -> Self {
        AtprotoStoreError { message: e.to_string() }
    }
}

pub(crate) trait KeyValueBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError>;
    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError>;
    fn delete(&self, account: &str) -> Result<(), AtprotoStoreError>;
}

pub(crate) struct KeyringBackend;

impl KeyValueBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        let entry = Entry::new("net.olamaelcu.livtet.atproto", account)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        let entry = Entry::new("net.olamaelcu.livtet.atproto", account)?;
        entry.set_password(value)?;
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), AtprotoStoreError> {
        let entry = Entry::new("net.olamaelcu.livtet.atproto", account)?;
        entry.delete_credential()?;
        Ok(())
    }
}

pub(crate) type StateMap = BTreeMap<String, atrium_oauth::store::state::InternalStateData>;
pub(crate) type SessionMap = BTreeMap<String, atrium_oauth::store::session::Session>;

pub(crate) struct AtprotoStateStore {
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
}

pub(crate) struct AtprotoSessionStore {
    backend: Arc<dyn KeyValueBackend>,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl Clone for AtprotoStateStore {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            lock: self.lock.clone(),
        }
    }
}

impl Clone for AtprotoSessionStore {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            lock: self.lock.clone(),
        }
    }
}

impl AtprotoStateStore {
    pub(crate) fn new(backend: Arc<dyn KeyValueBackend>) -> Self {
        Self {
            backend,
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(crate) async fn clear(&self) -> Result<(), AtprotoStoreError> {
        self.lock.lock().await;
        self.backend.delete("sessions")?;
        self.backend.delete("state")?;
        self.lock.unlock().await;
        Ok(())
    }

    pub(crate) async fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        self.lock.lock().await;
        self.backend.set(account, value)?;
        self.lock.unlock().await;
        Ok(())
    }

    pub(crate) async fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        self.lock.lock().await;
        let result = self.backend.get(account)?;
        self.lock.unlock().await;
        Ok(result)
    }
}

impl AtprotoSessionStore {
    pub(crate) fn new(backend: Arc<dyn KeyValueBackend>) -> Self {
        Self {
            backend,
            lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub(crate) async fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        self.lock.lock().await;
        self.backend.set(account, value)?;
        self.lock.unlock().await;
        Ok(())
    }

    pub(crate) async fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        self.lock.lock().await;
        let result = self.backend.get(account)?;
        self.lock.unlock().await;
        Ok(result)
    }

    pub(crate) async fn delete(&self, account: &str) -> Result<(), AtprotoStoreError> {
        self.lock.lock().await;
        self.backend.delete(account)?;
        self.lock.unlock().await;
        Ok(())
    }
}

pub(crate) async fn get_current_did(
    backend: Arc<dyn KeyValueBackend>,
) -> Result<Option<String>, AtprotoStoreError> {
    let entry = Entry::new("net.olamaelcu.livtet.atproto", "current")?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub(crate) async fn set_current_did(
    backend: Arc<dyn KeyValueBackend>,
    did: &str,
) -> Result<(), AtprotoStoreError> {
    let entry = Entry::new("net.olamaelcu.livtet.atproto", "current")?;
    entry.set_password(did)?;
    Ok(())
}

pub(crate) async fn clear_current_did(
    backend: Arc<dyn KeyValueBackend>,
) -> Result<(), AtprotoStoreError> {
    let entry = Entry::new("net.olamaelcu.livtet.atproto", "current")?;
    entry.delete_credential()?;
    Ok(())
}

impl atrium_oauth::store::state::StateStore for AtprotoStateStore {
    type Error = AtprotoStoreError;

    async fn set(&self, key: String, value: atrium_oauth::store::state::InternalStateData) -> Result<(), Self::Error> {
        let json = serde_json::to_string(&value).map_err(|e| AtprotoStoreError { message: e.to_string() })?;
        self.set(&key, &json).await
    }

    async fn get(&self, key: &str) -> Result<Option<atrium_oauth::store::state::InternalStateData>, Self::Error> {
        let json = self.get(key).await?;
        Ok(json.map(|j| serde_json::from_str(&j).unwrap()))
    }

    async fn delete(&self, key: &str) -> Result<(), Self::Error> {
        self.delete(key).await
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        self.clear().await
    }
}

impl atrium_oauth::store::session::SessionStore for AtprotoSessionStore {
    type Error = AtprotoStoreError;

    async fn set(&self, key: String, value: atrium_oauth::store::session::Session) -> Result<(), Self::Error> {
        let json = serde_json::to_string(&value).map_err(|e| AtprotoStoreError { message: e.to_string() })?;
        self.set(&key, &json).await
    }

    async fn get(&self, key: &str) -> Result<Option<atrium_oauth::store::session::Session>, Self::Error> {
        let json = self.get(key).await?;
        Ok(json.map(|j| serde_json::from_str(&j).unwrap()))
    }

    async fn delete(&self, key: &str) -> Result<(), Self::Error> {
        self.delete(key).await
    }

    async fn clear(&self) -> Result<(), Self::Error> {
        self.backend.delete("sessions")?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) struct MemoryBackend {
    store: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

#[cfg(test)]
impl MemoryBackend {
    pub(crate) fn new() -> Self {
        Self {
            store: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

#[cfg(test)]
impl KeyValueBackend for MemoryBackend {
    fn get(&self, account: &str) -> Result<Option<String>, AtprotoStoreError> {
        Ok(self.store.lock().unwrap().get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), AtprotoStoreError> {
        self.store.lock().unwrap().insert(account.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), AtprotoStoreError> {
        self.store.lock().unwrap().remove(account);
        Ok(())
    }
}
