use thiserror::Error;

/// Errors returned while opening or reading an EPUB for the reader.
///
/// Opening fails closed: an unreadable archive or a package document with no
/// reading order is an error, never a partially usable [`crate::Reader`].
#[derive(Debug, Error)]
pub enum ReaderError {
    #[error("I/O error while reading EPUB: {0}")]
    Io(#[from] std::io::Error),

    #[error("malformed EPUB archive: {0}")]
    Archive(#[from] livtet_epub::EpubError),

    #[error("invalid EPUB package document: {0}")]
    Package(String),

    #[error("unsupported EPUB: {0}")]
    Unsupported(String),
}
