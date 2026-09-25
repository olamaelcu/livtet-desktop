//! XMP metadata extraction.

use pdf_oxide::PdfDocument;
use pdf_oxide::extractors::xmp::{XmpExtractor, XmpMetadata};

use crate::error::PdfError;

/// Extract the document's XMP packet, if one is present.
pub(crate) fn extract(doc: &PdfDocument) -> Result<Option<XmpMetadata>, PdfError> {
    Ok(XmpExtractor::extract(doc)?)
}
