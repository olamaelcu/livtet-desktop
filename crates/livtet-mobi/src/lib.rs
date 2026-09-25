//! # Livtet MOBI
//!
//! Parse MOBI-family files (MOBI6 and KF8/AZW3, backing both `.azw` and
//! `.azw3`) and extract bibliographic metadata for import into Livtet. KFX
//! and Topaz are different container families and are rejected; the
//! hand-rolled parser in this module tree handles the PalmDB wrapper,
//! PalmDOC/MOBI headers, and the EXTH metadata block with no third-party
//! MOBI library involved.
//!
//! Extraction is fail-closed: an unreadable file and a missing title or
//! creator are errors, never partial records. There is deliberately no text
//! or HTML extraction — metadata and cover only. A non-zero PalmDOC
//! encryption type is recorded, not rejected: DRM encrypts text records
//! while EXTH metadata and cover images stay plaintext.

mod cover;
mod encoding;
mod error;
mod header;
mod metadata;
mod pdb;

#[cfg(test)]
mod test_support;

use std::path::Path;

pub use error::MobiError;
pub use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, Language, PublicationDate, Publisher, Role,
    SourceMetadata, Subject, Title,
};
pub use livtet_types::Isbn;

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, MobiError>;

/// Open `path` as a MOBI-family file and extract bibliographic metadata.
///
/// Fails closed: an unreadable file or a missing title or creator is an
/// error. A missing ISBN is not: [`SourceMetadata::isbns`] may be empty while
/// non-ISBN identifiers are preserved in `other_identifiers`.
pub fn read_metadata(path: &Path) -> Result<SourceMetadata> {
    let bytes = std::fs::read(path)?;
    metadata::extract(&bytes)
}
