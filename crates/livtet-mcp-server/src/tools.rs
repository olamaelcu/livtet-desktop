//! The MCP tool surface: one tool per exposed desktop command.
//!
//! Each tool is a thin translation layer — parameters in, JSON out — over
//! [`DesktopBridge`]. The router is the allowlist: a command that is not a
//! `#[tool]` method in this file is unreachable over MCP, so adding a command
//! to the endpoint is a deliberate edit here rather than a registry entry that
//! can drift.

use rmcp::{
    Json, ServerHandler,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::DesktopBridge;

/// Result shape every tool shares: structured JSON on both paths. The error
/// branch carries `{ code, message }` and is flagged `isError` by the SDK.
type ToolResult = Result<Json<Value>, Json<Value>>;

/// Turn a tool result into the MCP response, preserving the error payload.
fn finish(result: Result<Value, crate::BridgeError>) -> ToolResult {
    result.map(Json).map_err(|error| Json(error.to_value()))
}

/// Arguments for [`LivtetMcpServer::search_typeahead`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchTypeaheadArgs {
    /// Text the user typed into the library search field.
    pub query: String,
    /// Maximum number of hits to return; the desktop default applies when absent.
    #[serde(default)]
    pub limit: Option<i32>,
}

/// Arguments for [`LivtetMcpServer::search_editions`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SearchEditionsArgs {
    /// Free-text query; omit or leave blank to browse with filters only.
    #[serde(default)]
    pub query: Option<String>,
    /// Filter set with the same shape as the desktop `EditionFilters`:
    /// arrays of id strings for `tag_ids`, `genre_ids`, `subject_ids`,
    /// `publisher_ids`, `author_ids`, `format_ids`, `language_ids`, plus
    /// optional `has_file`, `sort_by`, and `sort_direction`.
    #[serde(default)]
    pub filters: Option<Value>,
    /// Rows to skip; defaults to the desktop page start.
    #[serde(default)]
    pub offset: Option<i32>,
    /// Rows to return; defaults to the desktop page size.
    #[serde(default)]
    pub limit: Option<i32>,
}

/// Arguments for [`LivtetMcpServer::import_file`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportFileArgs {
    /// Absolute path to the book file to import.
    pub path: String,
    /// `link` (symlink into the library, the default) or `copy`.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Arguments for [`LivtetMcpServer::import_files`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportFilesArgs {
    /// Absolute paths to the book files to import, in order.
    pub paths: Vec<String>,
    /// `link` (symlink into the library, the default) or `copy`.
    #[serde(default)]
    pub mode: Option<String>,
}

/// Arguments for [`LivtetMcpServer::create_virtual_book`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct CreateVirtualBookArgs {
    /// Catalog metadata with the same shape as the desktop `VirtualBookInput`:
    /// `title`, `authors` (`[{ "name": ..., "role": ... }]`), optional
    /// `publisher`, `language_id`, `format_id`, `identifiers`, `description`,
    /// and `notes`.
    pub input: Value,
}

/// Arguments for tools keyed by an edition id.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct EditionArgs {
    /// Edition id, exactly as returned by `search_editions`.
    pub edition_id: String,
}

/// Arguments for [`LivtetMcpServer::reader_resource`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ReaderResourceArgs {
    /// Edition id, exactly as returned by `search_editions`.
    pub edition_id: String,
    /// Publication-relative href of the resource, e.g. `OEBPS/chapter1.xhtml`.
    pub href: String,
}

/// Arguments for [`LivtetMcpServer::save_reader_position`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SaveReaderPositionArgs {
    /// Edition id, exactly as returned by `search_editions`.
    pub edition_id: String,
    /// Reader family: `epub`, `pdf`, or `audio`.
    pub kind: String,
    /// EPUB locator JSON, for `kind = "epub"`.
    #[serde(default)]
    pub locator: Option<String>,
    /// 1-based page number, for `kind = "pdf"`.
    #[serde(default)]
    pub page_number: Option<u32>,
    /// Playback position in seconds, for `kind = "audio"`.
    #[serde(default)]
    pub position_seconds: Option<f64>,
    /// Reader annotations to persist alongside the position.
    #[serde(default)]
    pub annotations: Option<String>,
}

/// Arguments for [`LivtetMcpServer::add_reader_bookmark`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct AddReaderBookmarkArgs {
    /// Edition id, exactly as returned by `search_editions`.
    pub edition_id: String,
    /// Bookmark kind: `epub`, `pdf`, `page`, or `audio`.
    pub kind: String,
    /// EPUB locator JSON, for `kind = "epub"`.
    #[serde(default)]
    pub locator: Option<String>,
    /// 1-based page number, for `kind = "pdf"` or `kind = "page"`.
    #[serde(default)]
    pub page_number: Option<u32>,
    /// Playback position in seconds, for `kind = "audio"`.
    #[serde(default)]
    pub position_seconds: Option<f64>,
    /// Fraction of the publication the location names, for `kind = "epub"`.
    #[serde(default)]
    pub fraction: Option<f64>,
}

/// Arguments for [`LivtetMcpServer::remove_reader_bookmark`].
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct RemoveReaderBookmarkArgs {
    /// Bookmark id, exactly as returned by `list_reader_bookmarks`.
    pub bookmark_id: String,
}

/// The MCP server: one tool per [`DesktopBridge`] method.
///
/// Cloned per session by the transport, so a client never observes another
/// client's session state.
#[derive(Debug, Clone)]
pub struct LivtetMcpServer<B: DesktopBridge> {
    bridge: B,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl<B: DesktopBridge> LivtetMcpServer<B> {
    /// Wrap a bridge in a server with every tool registered.
    pub fn new(bridge: B) -> Self {
        Self {
            bridge,
            tool_router: Self::tool_router(),
        }
    }

    /// Instant title/author matches for a typed query (`searchTypeahead`).
    #[tool(
        name = "search_typeahead",
        description = "Instant library title/author matches for a typed query."
    )]
    async fn search_typeahead(
        &self,
        Parameters(args): Parameters<SearchTypeaheadArgs>,
    ) -> ToolResult {
        finish(self.bridge.search_typeahead(args.query, args.limit).await)
    }

    /// The paginated library search (`searchEditions`).
    #[tool(
        name = "search_editions",
        description = "Search the library by text and filters; returns hits and a total."
    )]
    async fn search_editions(
        &self,
        Parameters(args): Parameters<SearchEditionsArgs>,
    ) -> ToolResult {
        finish(
            self.bridge
                .search_editions(args.query, args.filters, args.offset, args.limit)
                .await,
        )
    }

    /// Every filter axis the search UI can offer (`filterOptions`).
    #[tool(
        name = "filter_options",
        description = "List every filter value the library can be searched by."
    )]
    async fn filter_options(&self) -> ToolResult {
        finish(self.bridge.filter_options().await)
    }

    /// Import one file into the library (`importFile`).
    #[tool(
        name = "import_file",
        description = "Import one book file into the library and index it."
    )]
    async fn import_file(&self, Parameters(args): Parameters<ImportFileArgs>) -> ToolResult {
        let mode = args.mode.unwrap_or_else(|| String::from("link"));
        finish(self.bridge.import_file(args.path, mode).await)
    }

    /// Import several files, returning the aggregate result (`importFiles`).
    #[tool(
        name = "import_files",
        description = "Import several book files in order; returns per-file results."
    )]
    async fn import_files(&self, Parameters(args): Parameters<ImportFilesArgs>) -> ToolResult {
        let mode = args.mode.unwrap_or_else(|| String::from("link"));
        finish(self.bridge.import_files(args.paths, mode).await)
    }

    /// Catalog metadata for a book with no file on disk (`createVirtualBook`).
    #[tool(
        name = "create_virtual_book",
        description = "Create a catalog-only book: no file on disk, searchable and browsable."
    )]
    async fn create_virtual_book(
        &self,
        Parameters(args): Parameters<CreateVirtualBookArgs>,
    ) -> ToolResult {
        finish(self.bridge.create_virtual_book(args.input).await)
    }

    /// The reader descriptor for one edition (`readerPublication`).
    #[tool(
        name = "reader_publication",
        description = "Describe an edition for its reader: EPUB manifest, PDF URL, or audiobook chapters."
    )]
    async fn reader_publication(&self, Parameters(args): Parameters<EditionArgs>) -> ToolResult {
        finish(self.bridge.reader_publication(args.edition_id).await)
    }

    /// One EPUB publication resource as UTF-8 text (`readerResource`).
    #[tool(
        name = "reader_resource",
        description = "Read one EPUB resource (a chapter, say) as UTF-8 text."
    )]
    async fn reader_resource(
        &self,
        Parameters(args): Parameters<ReaderResourceArgs>,
    ) -> ToolResult {
        finish(
            self.bridge
                .reader_resource(args.edition_id, args.href)
                .await,
        )
    }

    /// The persisted reading position for an edition (`getReaderPosition`).
    #[tool(
        name = "get_reader_position",
        description = "Read the saved reading position for an edition."
    )]
    async fn get_reader_position(&self, Parameters(args): Parameters<EditionArgs>) -> ToolResult {
        finish(self.bridge.get_reader_position(args.edition_id).await)
    }

    /// Upsert a reading position (`saveReaderPosition`).
    #[tool(
        name = "save_reader_position",
        description = "Save a reading position: an EPUB locator, a PDF page, or playback seconds."
    )]
    async fn save_reader_position(
        &self,
        Parameters(args): Parameters<SaveReaderPositionArgs>,
    ) -> ToolResult {
        finish(
            self.bridge
                .save_reader_position(
                    args.edition_id,
                    args.kind,
                    args.locator,
                    args.page_number,
                    args.position_seconds,
                    args.annotations,
                )
                .await,
        )
    }

    /// Every bookmark on an edition, oldest first (`listReaderBookmarks`).
    #[tool(
        name = "list_reader_bookmarks",
        description = "List an edition's bookmarks, oldest first."
    )]
    async fn list_reader_bookmarks(&self, Parameters(args): Parameters<EditionArgs>) -> ToolResult {
        finish(self.bridge.list_reader_bookmarks(args.edition_id).await)
    }

    /// Bookmark an edition at a location (`addReaderBookmark`).
    #[tool(
        name = "add_reader_bookmark",
        description = "Bookmark an edition's current location; a bookmark carries no note text."
    )]
    async fn add_reader_bookmark(
        &self,
        Parameters(args): Parameters<AddReaderBookmarkArgs>,
    ) -> ToolResult {
        finish(
            self.bridge
                .add_reader_bookmark(
                    args.edition_id,
                    args.kind,
                    args.locator,
                    args.page_number,
                    args.position_seconds,
                    args.fraction,
                )
                .await,
        )
    }

    /// Delete one bookmark by id (`removeReaderBookmark`).
    #[tool(
        name = "remove_reader_bookmark",
        description = "Remove one bookmark; reports whether a row was deleted."
    )]
    async fn remove_reader_bookmark(
        &self,
        Parameters(args): Parameters<RemoveReaderBookmarkArgs>,
    ) -> ToolResult {
        finish(self.bridge.remove_reader_bookmark(args.bookmark_id).await)
    }
}

#[tool_handler(
    router = self.tool_router,
    name = "livtet-desktop",
    instructions = "Tools for the Livtet desktop library: search the catalog, import books, and read or bookmark them. Everything runs against the local library on this machine."
)]
impl<B: DesktopBridge> ServerHandler for LivtetMcpServer<B> {}
