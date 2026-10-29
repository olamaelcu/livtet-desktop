//! Contract between the MCP endpoint and the desktop command layer.
//!
//! `livtet-desktop` implements [`DesktopBridge`] over its live `AppState`.
//! This crate links neither Tauri nor the catalog model, so the whole tool
//! surface can be exercised against a stub bridge in tests, and the desktop
//! crate stays the only place that knows how a command is really executed.

use std::{error::Error, fmt, future::Future};

use serde_json::Value;

/// Failure reported to an MCP client.
///
/// Mirrors the `{ code, message }` shape the desktop commands already return
/// over IPC, so a client can branch on `code` the same way the webview does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeError {
    /// Stable machine-readable reason (`search`, `import`, `reader`, ...).
    pub code: String,
    /// Human-readable detail.
    pub message: String,
}

impl BridgeError {
    /// Build an error from a stable code and a human-readable message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    /// The structured payload returned as tool error content.
    pub fn to_value(&self) -> Value {
        serde_json::json!({ "code": self.code, "message": self.message })
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl Error for BridgeError {}

/// The desktop commands reachable over MCP.
///
/// Methods mirror the Tauri command signatures they serve, minus the Tauri
/// state and app handles, and return the command's serialized result. Every
/// future is `Send` because the MCP transport drives handlers from a
/// multi-threaded runtime.
pub trait DesktopBridge: Clone + Send + Sync + 'static {
    /// `search_typeahead`: instant title/author matches for a typed query.
    fn search_typeahead(
        &self,
        query: String,
        limit: Option<i32>,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `search_editions`: the paginated library search.
    fn search_editions(
        &self,
        query: Option<String>,
        filters: Option<Value>,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `filter_options`: every filter axis the search UI can offer.
    fn filter_options(&self) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `import_file`: import one file into the library.
    fn import_file(
        &self,
        path: String,
        mode: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `import_files`: import several files, returning the aggregate result.
    fn import_files(
        &self,
        paths: Vec<String>,
        mode: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `create_virtual_book`: catalog metadata with no file on disk.
    fn create_virtual_book(
        &self,
        input: Value,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `reader_publication`: the reader descriptor for one edition.
    fn reader_publication(
        &self,
        edition_id: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `reader_resource`: one EPUB publication resource as UTF-8 text.
    fn reader_resource(
        &self,
        edition_id: String,
        href: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `get_reader_position`: the persisted reading position for an edition.
    fn get_reader_position(
        &self,
        edition_id: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `save_reader_position`: upsert a reading position.
    #[allow(clippy::too_many_arguments)]
    fn save_reader_position(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        annotations: Option<String>,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `list_reader_bookmarks`: every bookmark on an edition, oldest first.
    fn list_reader_bookmarks(
        &self,
        edition_id: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `add_reader_bookmark`: bookmark an edition at a location.
    fn add_reader_bookmark(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        fraction: Option<f64>,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;

    /// `remove_reader_bookmark`: delete one bookmark by id.
    fn remove_reader_bookmark(
        &self,
        bookmark_id: String,
    ) -> impl Future<Output = Result<Value, BridgeError>> + Send;
}
