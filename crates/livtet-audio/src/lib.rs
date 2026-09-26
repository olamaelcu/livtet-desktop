//! # Livtet Audio
//!
//! Parse iTunes MP4 audiobooks (`.m4b`, `.m4a`) and extract bibliographic
//! metadata, cover art, duration, and chapters for import into Livtet.
//! `mp4ameta` reads the iTunes item list, both chapter mechanisms (chapter
//! track first, `chpl` list as fallback), artwork, and audio properties.
//!
//! Extraction is fail-closed: an unreadable file and a missing title or
//! creator are errors, never partial records. Audio samples are never decoded
//! — metadata and cover only. There is deliberately no DRM handling: retail
//! audiobook MP4s carry their metadata and cover in the clear.

mod error;
mod metadata;

use std::path::Path;

pub use error::AudioError;
pub use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, Language, PublicationDate, Publisher, Role,
    SourceMetadata, Subject, Title,
};
pub use livtet_types::Isbn;

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, AudioError>;

/// Open `path` as an iTunes MP4 audiobook and extract bibliographic metadata.
///
/// Fails closed: an unreadable file or a missing title or creator is an
/// error. A missing ISBN is not: [`SourceMetadata::isbns`] is empty for
/// audiobooks while an ASIN-style identifier is preserved in
/// `other_identifiers` when present.
pub fn read_metadata(path: &Path) -> Result<SourceMetadata> {
    let tag = mp4ameta::Tag::read_from_path(path)?;
    let title_fallback = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    metadata::from_tag(&tag, title_fallback.as_deref())
}
