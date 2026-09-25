//! # Livtet EPUB
//!
//! Parse EPUB 2/3 files and extract bibliographic metadata for import into
//! Livtet. The container (OCF), package document (OPF), and encryption
//! metadata are read by the tolerant in-crate parser in this module tree; no
//! third-party EPUB library is involved.
//!
//! Extraction is fail-closed: an unreadable archive, a missing title or
//! creator, and the absence of any valid ISBN are errors, never partial
//! records.
//!
//! Contributors from `dc:creator`/`dc:contributor` are de-duplicated by
//! `(name, role)`; a role-less contributor that repeats a creator is dropped.
//! [`EpubMetadata::title_sort`] comes from the main title's `file-as`
//! refinement or `calibre:title_sort`.

mod cover;
mod encryption;
mod error;
mod metadata;
mod ocf;
mod xml;
mod zip;

#[cfg(test)]
mod test_support;

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
/// Fails closed: an unreadable file, a missing title or creator, or the
/// absence of at least one valid ISBN anywhere in the metadata is an error.
pub fn read_metadata(path: &Path) -> Result<EpubMetadata> {
    let bytes = std::fs::read(path)?;
    let mut archive = zip::Archive::open(bytes)?;
    metadata::extract(&mut archive)
}
