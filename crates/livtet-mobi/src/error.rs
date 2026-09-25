use thiserror::Error;

/// Errors returned while reading a MOBI-family file.
///
/// Every failure is explicit: the parser never fabricates empty metadata or
/// silently drops a file it could not read.
///
/// Note: there is deliberately no DRM variant. Only the text records of a
/// DRM'd file are encrypted; EXTH metadata and cover images stay plaintext,
/// so parsing proceeds normally and records the encryption type instead of
/// rejecting the file.
#[derive(Debug, Error)]
pub enum MobiError {
    #[error("I/O error while reading MOBI: {0}")]
    Io(#[from] std::io::Error),

    #[error("not a MOBI-family file")]
    NotMobi,

    #[error("unsupported MOBI-family container: {0}")]
    UnsupportedContainer(&'static str),

    #[error("malformed MOBI file: {0}")]
    Malformed(String),

    #[error("missing required metadata: {0}")]
    MissingRequired(&'static str),
}
