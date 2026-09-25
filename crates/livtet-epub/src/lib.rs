//! # Livtet EPUB
//!
//! Parse EPUB 2/3 files and extract bibliographic metadata for import into
//! Livtet. The container (OCF), package document (OPF), and encryption
//! metadata are read by the tolerant in-crate parser in this module tree; no
//! third-party EPUB library is involved.
//!
//! Extraction is fail-closed: an unreadable archive and a missing title or
//! creator are errors, never partial records. An ISBN is optional: when the
//! metadata declares none, the content documents are scanned (spine first) for
//! one, and `isbns` may legitimately be empty. Non-ISBN identifiers (UUID,
//! ASIN, publisher ids) are preserved in `other_identifiers` so an edition
//! without an ISBN still has identity.
//!
//! Contributors from `dc:creator`/`dc:contributor` are de-duplicated by
//! `(name, role)`; a role-less contributor that repeats a creator is dropped.
//! [`SourceMetadata::title_sort`] comes from the main title's `file-as`
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

pub use error::EpubError;
pub use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, Language, PublicationDate, Publisher, Role,
    SourceMetadata, Subject, Title,
};
pub use livtet_types::Isbn;

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, EpubError>;

/// Open `path` as an EPUB and extract bibliographic metadata.
///
/// Fails closed: an unreadable file or a missing title or creator is an error.
/// A missing ISBN is not: when the metadata carries none, the content documents
/// are scanned for one, so [`SourceMetadata::isbns`] may be empty.
pub fn read_metadata(path: &Path) -> Result<SourceMetadata> {
    let bytes = std::fs::read(path)?;
    let mut archive = zip::Archive::open(bytes)?;
    metadata::extract(&mut archive)
}
