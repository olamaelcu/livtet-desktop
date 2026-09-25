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
pub enum ReaderError {
    #[error("Unknown edition: {id}")]
    UnknownEdition { id: String },

    #[error("Edition has no file")]
    NoFile,

    #[error("Edition file is missing")]
    MissingFile,

    #[error("Unsupported format: {format}")]
    UnsupportedFormat { format: String },

    #[error("Publication error: {message}")]
    Publication { message: String },
}

impl ReaderError {
    pub fn unknown_edition(id: impl Into<String>) -> Self {
        Self::UnknownEdition { id: id.into() }
    }

    pub fn unsupported_format(format: impl Into<String>) -> Self {
        Self::UnsupportedFormat {
            format: format.into(),
        }
    }

    pub fn publication(message: impl std::fmt::Display) -> Self {
        Self::Publication {
            message: message.to_string(),
        }
    }
}

impl From<livtet_reader::ReaderError> for ReaderError {
    fn from(err: livtet_reader::ReaderError) -> Self {
        Self::publication(err)
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
#[derive(Debug, Clone, Error, Serialize, Type)]
pub enum OpdsError {
    #[error("Invalid catalog URL: {message}")]
    InvalidUrl { message: String },

    #[error("Invalid OPDS input: {message}")]
    InvalidInput { message: String },

    #[error("Catalog not found: {id}")]
    NotFound { id: String },

    #[error("Refusing to send credentials insecurely: {message}")]
    InsecureCredentials { message: String },

    #[error("OPDS credentials unavailable: {message}")]
    CredentialsUnavailable { message: String },

    #[error("OPDS request failed: {message}")]
    Network { message: String },

    #[error("OPDS server returned status {status}")]
    Http { status: i32 },

    #[error("OPDS feed error: {message}")]
    Feed { message: String },

    #[error("OPDS catalog storage error: {message}")]
    Storage { message: String },

    #[error("Import failed ({code}): {message}")]
    Import { code: String, message: String },
}

impl From<crate::secrets::SecretError> for OpdsError {
    fn from(error: crate::secrets::SecretError) -> Self {
        match error {
            crate::secrets::SecretError::Unavailable(message) => {
                Self::CredentialsUnavailable { message }
            }
            crate::secrets::SecretError::Malformed(message) => Self::Storage { message },
        }
    }
}

impl OpdsError {
    pub fn invalid_input(message: impl std::fmt::Display) -> Self {
        Self::InvalidInput {
            message: message.to_string(),
        }
    }

    pub fn feed(message: impl std::fmt::Display) -> Self {
        Self::Feed {
            message: message.to_string(),
        }
    }

    pub fn network<E: std::fmt::Display>(err: E) -> Self {
        Self::Network {
            message: err.to_string(),
        }
    }

    pub fn storage<E: std::fmt::Display>(err: E) -> Self {
        Self::Storage {
            message: err.to_string(),
        }
    }
}

impl From<livtet_opds_types::OpdsError> for OpdsError {
    fn from(err: livtet_opds_types::OpdsError) -> Self {
        use livtet_opds_types::OpdsError as Opds;
        match err {
            Opds::Http(status) => Self::Http {
                status: i32::from(status),
            },
            Opds::Network(message) => Self::Network { message },
            Opds::InvalidUrl(err) => Self::InvalidUrl {
                message: err.to_string(),
            },
            Opds::MissingField(field) => Self::Feed {
                message: format!("missing field: {field}"),
            },
            other => Self::Feed {
                message: other.to_string(),
            },
        }
    }
}
