use thiserror::Error;

/// Errors returned while reading an EPUB container.
///
/// Every failure is explicit: the parser never fabricates empty metadata or
/// silently drops a file it could not read.
#[derive(Debug, Error)]
pub enum EpubError {
    #[error("I/O error while reading EPUB: {0}")]
    Io(#[from] std::io::Error),

    #[error("malformed XML: {0}")]
    Xml(String),

    #[error("malformed ZIP archive: {0}")]
    Zip(String),

    #[error("invalid OCF container: {0}")]
    Container(String),

    #[error("missing required metadata: {0}")]
    MissingRequired(&'static str),

    #[error("no valid ISBN found in metadata (ISBNs are required for import)")]
    MissingIsbn,
}
