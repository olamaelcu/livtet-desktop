//! # Livtet EPUB
//!
//! Parse EPUB 2/3 files and extract bibliographic metadata for import into
//! Livtet. Parsing is delegated to the [`epub`] crate; this crate layers
//! Livtet's normalization and fail-closed requirements on top.
//!
//! [`epub`]: https://docs.rs/epub

mod cover;
mod error;
mod metadata;

use epub::doc::EpubDoc;
use std::path::Path;

pub use cover::Cover;
pub use error::EpubError;
pub use livtet_types::Isbn;
pub use metadata::{
    Contributor, Description, EpubMetadata, Identifier, Language, PublicationDate, Publisher, Role,
    Subject, Title,
};

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, EpubError>;

/// Open `path` as an EPUB and extract bibliographic metadata.
///
/// Fails closed: an unreadable file, a missing title, or the absence of at
/// least one valid ISBN anywhere in the metadata is an error.
pub fn read_metadata(path: &Path) -> Result<EpubMetadata> {
    let mut doc = EpubDoc::new(path)?;
    metadata::extract(&mut doc)
}
