use serde::Serialize;
use thiserror::Error;
use specta::Type;

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
        Self::Other { message: err.to_string() }
    }
}