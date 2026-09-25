//! # Livtet PDF
//!
//! Parse PDF documents and extract bibliographic metadata for import into
//! Livtet. All-in-one extraction is delegated to [`pdf_oxide`] (Info
//! dictionary, XMP packet, and embedded images), with [`hayro`] rendering page
//! 0 only as a cover fallback when the document carries no usable image.
//!
//! Extraction is fail-closed: an unreadable document and a missing title or
//! creator are errors, never partial records. An ISBN is optional — it may
//! legitimately be empty, and non-ISBN candidates are preserved as
//! `other_identifiers` so an edition still has identity. Recovery is
//! metadata-only: the Info `/Keywords` and `/Subject`, and the XMP
//! `pdf:Keywords`, `dc:subject`, and custom properties are scanned. Page text
//! is never extracted.
//!
//! The cover is the largest embedded image painted on page 0 (JPEG passed
//! through byte-for-byte, everything else re-encoded to PNG), falling back to a
//! white-backed render of page 0. A missing or unreadable cover is [`None`],
//! never an error and never ciphertext.
//!
//! ## Known limitations
//!
//! * `publisher` is always [`None`]: PDF has no standard publisher field, and
//!   `pdf_oxide` exposes none in either carrier.
//! * `published` is the document's creation date (XMP first, then Info), which
//!   is the file's date, not necessarily the work's publication date.
//! * An ISBN is only discovered in metadata keywords; it is never read from
//!   the page text.

mod cover;
mod error;
mod info;
mod metadata;
mod xmp;

use std::path::Path;

pub use error::PdfError;
pub use livtet_importer_types::{
    Contributor, Cover, Description, Identifier, Language, PublicationDate, Publisher, Role,
    SourceMetadata, Subject, Title,
};
pub use livtet_types::Isbn;
pub use metadata::PdfMetadata;

/// Convenience alias for results from this crate.
pub type Result<T> = std::result::Result<T, PdfError>;

/// Open `path` as a PDF and extract bibliographic metadata.
///
/// Fails closed: an unreadable, encrypted, or malformed file and a missing
/// title or creator are errors. A missing ISBN and a missing cover are not:
/// [`PdfMetadata::isbns`] may be empty and [`PdfMetadata::cover`] may be
/// [`None`].
pub fn read_metadata(path: &Path) -> Result<PdfMetadata> {
    let bytes = std::fs::read(path)?;
    let doc = pdf_oxide::PdfDocument::from_bytes(bytes.clone())?;
    // XMP is supplementary: a document whose XMP packet cannot be read still
    // has its Info dictionary, so an XMP failure degrades to "no XMP".
    let xmp = xmp::extract(&doc).ok().flatten();
    let info = info::extract(path)?;
    let cover = cover::extract(&doc, &bytes);
    metadata::merge(info, xmp, cover)
}
