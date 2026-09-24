use serde::Serialize;
use specta::Type;
use thiserror::Error;

#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum SearchIndexError {
    #[error("Search index unavailable: {0}")]
    Unavailable(String),

    #[error("Search index error: {message}")]
    Other { message: String },
}

impl SearchIndexError {
    pub fn unavailable(msg: &'static str) -> Self {
        Self::Unavailable(msg.to_string())
    }

    pub fn other<E: std::error::Error + Send + Sync + 'static>(err: E) -> Self {
        Self::Other {
            message: err.to_string(),
        }
    }
}
#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum CatalogError {
    #[error("Invalid edition id: {id}")]
    InvalidId { id: String },

    #[error("Catalog database error: {message}")]
    Database { message: String },
}

impl CatalogError {
    pub fn database<E: std::fmt::Display>(err: E) -> Self {
        Self::Database {
            message: err.to_string(),
        }
    }
}

#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum PluginError {
    #[error("Plugin host is unavailable: {message}")]
    Unavailable { message: String },

    #[error("Plugin host error: {message}")]
    Host { message: String },
}

impl PluginError {
    pub fn host<E: std::fmt::Display>(err: E) -> Self {
        Self::Host {
            message: err.to_string(),
        }
    }
}

#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum SyncError {
    #[error("Sync daemon is unavailable: {message}")]
    Unavailable { message: String },

    #[error("Sync daemon error: {message}")]
    Rpc { message: String },

    #[error("Sync protocol error: {message}")]
    Protocol { message: String },
}

impl SyncError {
    pub fn unavailable<E: std::fmt::Display>(err: E) -> Self {
        Self::Unavailable {
            message: err.to_string(),
        }
    }

    pub fn rpc<E: std::fmt::Display>(err: E) -> Self {
        Self::Rpc {
            message: err.to_string(),
        }
    }

    pub fn protocol<E: std::fmt::Display>(err: E) -> Self {
        Self::Protocol {
            message: err.to_string(),
        }
    }
}
