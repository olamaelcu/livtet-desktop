use thiserror::Error;

#[derive(Debug, Error)]
pub enum EpubError {
    #[error("failed to open or parse EPUB: {0}")]
    Doc(#[from] epub::doc::DocError),

    #[error("I/O error while reading EPUB: {0}")]
    Io(#[from] std::io::Error),

    #[error("missing required metadata: {0}")]
    MissingRequired(&'static str),

    #[error("no valid ISBN found in metadata (ISBNs are required for import)")]
    MissingIsbn,
}
