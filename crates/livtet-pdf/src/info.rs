//! Info-dictionary extraction.
//!
//! `pdf_oxide` exposes the trailer `/Info` dictionary only through its editing
//! API ([`DocumentEditor`] + [`EditableDocument::get_info`]), which needs the
//! document opened by path; the read-only [`PdfDocument`] has no `info()`.

use std::path::Path;

use pdf_oxide::editor::{DocumentEditor, DocumentInfo, EditableDocument};

use crate::error::PdfError;

/// Read the `/Info` dictionary, if the document carries one.
///
/// Returns [`DocumentInfo::default()`] when the document has no `/Info`
/// dictionary, so callers can treat "absent" and "empty" the same way.
pub(crate) fn extract(path: &Path) -> Result<DocumentInfo, PdfError> {
    let mut editor = DocumentEditor::open(path)?;
    Ok(editor.get_info()?)
}
