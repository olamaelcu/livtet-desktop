use thiserror::Error;

/// Errors returned while reading a PDF's metadata.
///
/// Every failure is explicit: extraction never fabricates empty metadata or
/// silently drops a file it could not read. A missing cover is *not* an error
/// (see [`crate::cover`]); a missing title or creator is.
#[derive(Debug, Error)]
pub enum PdfError {
    #[error("I/O error while reading PDF: {0}")]
    Io(#[from] std::io::Error),

    #[error("malformed PDF: {0}")]
    Malformed(String),

    #[error("encrypted PDF requires a password")]
    Encrypted,

    #[error("missing required metadata: {0}")]
    MissingRequired(&'static str),
}

impl From<pdf_oxide::error::Error> for PdfError {
    fn from(error: pdf_oxide::error::Error) -> Self {
        match error {
            pdf_oxide::error::Error::EncryptedPdf => Self::Encrypted,
            pdf_oxide::error::Error::Io(error) => Self::Io(error),
            other => Self::Malformed(other.to_string()),
        }
    }
}
