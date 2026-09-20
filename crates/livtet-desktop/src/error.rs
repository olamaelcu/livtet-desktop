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
