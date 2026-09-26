use thiserror::Error;

/// Errors returned while reading an iTunes MP4 audiobook.
///
/// Every failure is explicit: the parser never fabricates empty metadata or
/// silently drops a file it could not read.
#[derive(Debug, Error)]
pub enum AudioError {
    #[error("I/O error while reading audiobook: {0}")]
    Io(#[from] std::io::Error),

    #[error("audiobook container error: {0}")]
    Mp4(#[from] mp4ameta::Error),

    #[error("missing required metadata: {0}")]
    MissingRequired(&'static str),

    #[error("invalid audiobook metadata: {0}")]
    Invalid(String),
}
