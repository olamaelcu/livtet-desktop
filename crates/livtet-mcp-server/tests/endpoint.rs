//! End-to-end check: a real MCP client talks to the real endpoint over
//! loopback, and the endpoint refuses a forged `Host` header.
//!
//! These tests exercise the transport, the tool router, and the parameter
//! plumbing together; the desktop-side bridge gets its own tests in
//! `livtet-desktop`.
// Test assertions: a panic is the failure signal, so `expect` documents the
// invariant each step relies on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::{Arc, Mutex};

use livtet_mcp_server::{BridgeError, DesktopBridge, serve};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Every tool the endpoint is expected to publish.
const EXPECTED_TOOLS: [&str; 13] = [
    "add_reader_bookmark",
    "create_virtual_book",
    "filter_options",
    "get_reader_position",
    "import_file",
    "import_files",
    "list_reader_bookmarks",
    "reader_publication",
    "reader_resource",
    "remove_reader_bookmark",
    "save_reader_position",
    "search_editions",
    "search_typeahead",
];

/// A bridge that records every call and echoes its arguments back, so a test
/// can prove the transport carried each parameter to the handler.
#[derive(Clone, Default)]
struct StubBridge {
    calls: Arc<Mutex<Vec<String>>>,
}

impl StubBridge {
    fn called(&self, name: &str) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(name.to_owned());
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl DesktopBridge for StubBridge {
    async fn search_typeahead(
        &self,
        query: String,
        limit: Option<i32>,
    ) -> Result<Value, BridgeError> {
        self.called("search_typeahead");
        Ok(json!({ "query": query, "limit": limit }))
    }

    async fn search_editions(
        &self,
        query: Option<String>,
        filters: Option<Value>,
        offset: Option<i32>,
        limit: Option<i32>,
    ) -> Result<Value, BridgeError> {
        self.called("search_editions");
        Ok(json!({ "query": query, "filters": filters, "offset": offset, "limit": limit }))
    }

    async fn filter_options(&self) -> Result<Value, BridgeError> {
        self.called("filter_options");
        Ok(json!({ "axes": ["tags", "genres"] }))
    }

    async fn import_file(&self, path: String, mode: String) -> Result<Value, BridgeError> {
        self.called("import_file");
        Ok(json!({ "path": path, "mode": mode }))
    }

    async fn import_files(&self, paths: Vec<String>, mode: String) -> Result<Value, BridgeError> {
        self.called("import_files");
        Ok(json!({ "paths": paths, "mode": mode }))
    }

    async fn create_virtual_book(&self, input: Value) -> Result<Value, BridgeError> {
        self.called("create_virtual_book");
        Ok(json!({ "input": input }))
    }

    async fn reader_publication(&self, edition_id: String) -> Result<Value, BridgeError> {
        self.called("reader_publication");
        Ok(json!({ "edition_id": edition_id }))
    }

    async fn reader_resource(
        &self,
        edition_id: String,
        href: String,
    ) -> Result<Value, BridgeError> {
        self.called("reader_resource");
        Ok(json!({ "edition_id": edition_id, "href": href }))
    }

    async fn get_reader_position(&self, edition_id: String) -> Result<Value, BridgeError> {
        self.called("get_reader_position");
        Ok(json!({ "edition_id": edition_id }))
    }

    async fn save_reader_position(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        annotations: Option<String>,
    ) -> Result<Value, BridgeError> {
        self.called("save_reader_position");
        Ok(json!({
            "edition_id": edition_id,
            "kind": kind,
            "locator": locator,
            "page_number": page_number,
            "position_seconds": position_seconds,
            "annotations": annotations,
        }))
    }

    async fn list_reader_bookmarks(&self, edition_id: String) -> Result<Value, BridgeError> {
        self.called("list_reader_bookmarks");
        Ok(json!({ "edition_id": edition_id }))
    }

    async fn add_reader_bookmark(
        &self,
        edition_id: String,
        kind: String,
        locator: Option<String>,
        page_number: Option<u32>,
        position_seconds: Option<f64>,
        fraction: Option<f64>,
    ) -> Result<Value, BridgeError> {
        self.called("add_reader_bookmark");
        Ok(json!({
            "edition_id": edition_id,
            "kind": kind,
            "locator": locator,
            "page_number": page_number,
            "position_seconds": position_seconds,
            "fraction": fraction,
        }))
    }

    async fn remove_reader_bookmark(&self, bookmark_id: String) -> Result<Value, BridgeError> {
        self.called("remove_reader_bookmark");
        // One failing branch so the error path travels the whole wire.
        if bookmark_id == "bad" {
            return Err(BridgeError::new("invalid-id", "bookmark id is invalid"));
        }
        Ok(json!({ "bookmark_id": bookmark_id }))
    }
}

/// The concrete MCP client the endpoint is verified against.
type Client = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

/// Connect a real MCP client to a freshly started endpoint.
async fn connect(endpoint: &livtet_mcp_server::McpEndpoint) -> Client {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint.url()),
    );
    ().serve(transport)
        .await
        .expect("client connects to the endpoint")
}

/// A tool call's arguments as the wire shape the SDK expects.
fn arguments(value: Value) -> rmcp::model::JsonObject {
    value.as_object().cloned().expect("an object literal")
}

/// One tool call against a connected client, carrying a full argument object.
async fn call(client: &Client, name: &str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}

/// A tool whose handler hands every argument to the bridge untouched: the stub
/// echoes them back, so the structured result must equal the input exactly.
async fn expect_echoed(client: &Client, name: &str, args: Value) {
    let result = call(client, name, args.clone()).await;
    assert_eq!(result.is_error, Some(false), "{name} must report success");
    assert_eq!(
        result.structured_content,
        Some(args),
        "{name} must hand every argument to the bridge and echo it back"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn endpoint_publishes_the_curated_tool_set_on_loopback() {
    let endpoint = serve(StubBridge::default(), 0)
        .await
        .expect("bind loopback");
    assert!(
        endpoint.local_addr().ip().is_loopback(),
        "the endpoint must never bind a non-loopback address"
    );

    let client = connect(&endpoint).await;
    let listed = client.list_tools(None).await.expect("list tools");

    let mut names: Vec<String> = listed
        .tools
        .iter()
        .map(|tool| tool.name.to_string())
        .collect();
    names.sort();
    let mut expected: Vec<String> = EXPECTED_TOOLS
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    expected.sort();
    assert_eq!(names, expected);

    endpoint.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_arguments_reach_the_bridge_and_results_come_back() {
    let bridge = StubBridge::default();
    let endpoint = serve(bridge.clone(), 0).await.expect("bind loopback");
    let client = connect(&endpoint).await;

    let searched = client
        .call_tool(
            CallToolRequestParams::new("search_typeahead")
                .with_arguments(arguments(json!({ "query": "dune", "limit": 5 }))),
        )
        .await
        .expect("call search_typeahead");
    assert_eq!(searched.is_error, Some(false));
    assert_eq!(
        searched.structured_content,
        Some(json!({ "query": "dune", "limit": 5 }))
    );

    // A tool that takes no parameters at all still dispatches.
    let options = client
        .call_tool(CallToolRequestParams::new("filter_options"))
        .await
        .expect("call filter_options");
    assert_eq!(
        options.structured_content,
        Some(json!({ "axes": ["tags", "genres"] }))
    );

    assert_eq!(
        bridge.calls(),
        vec![
            String::from("search_typeahead"),
            String::from("filter_options")
        ]
    );

    endpoint.shutdown();
}

/// Per-tool contract check: one call per line of the [`DesktopBridge`] trait,
/// with every argument populated, so each tool handler in `tools.rs` runs and
/// the wire shape of every tool's parameters is pinned.
///
/// The stub echoes its arguments back, so equality with the raw arguments
/// proves both hops — transport to handler, then handler to bridge. The two
/// `mode`-less imports additionally pin the documented `"link"` default, and
/// the trailing call-order assertion fails on a dropped or duplicated dispatch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_tool_routes_its_arguments_to_the_bridge() {
    let bridge = StubBridge::default();
    let endpoint = serve(bridge.clone(), 0).await.expect("bind loopback");
    let client = connect(&endpoint).await;

    expect_echoed(
        &client,
        "search_typeahead",
        json!({ "query": "dune", "limit": 5 }),
    )
    .await;

    expect_echoed(
        &client,
        "search_editions",
        json!({
            "query": "dune",
            "filters": { "tag_ids": ["t1"] },
            "offset": 20,
            "limit": 5,
        }),
    )
    .await;

    // The one tool that takes no arguments returns a fixed payload.
    let options = client
        .call_tool(CallToolRequestParams::new("filter_options"))
        .await
        .expect("call filter_options");
    assert_eq!(options.is_error, Some(false), "filter_options must succeed");
    assert_eq!(
        options.structured_content,
        Some(json!({ "axes": ["tags", "genres"] })),
        "filter_options must return the bridge payload"
    );

    expect_echoed(
        &client,
        "import_file",
        json!({ "path": "/books/one.epub", "mode": "copy" }),
    )
    .await;

    expect_echoed(
        &client,
        "import_files",
        json!({ "paths": ["/books/a.epub", "/books/b.epub"], "mode": "copy" }),
    )
    .await;

    expect_echoed(
        &client,
        "create_virtual_book",
        json!({ "input": { "title": "Virtual" } }),
    )
    .await;

    expect_echoed(
        &client,
        "reader_publication",
        json!({ "edition_id": "ed-1" }),
    )
    .await;

    expect_echoed(
        &client,
        "reader_resource",
        json!({ "edition_id": "ed-1", "href": "OEBPS/ch1.xhtml" }),
    )
    .await;

    expect_echoed(
        &client,
        "get_reader_position",
        json!({ "edition_id": "ed-1" }),
    )
    .await;

    expect_echoed(
        &client,
        "save_reader_position",
        json!({
            "edition_id": "ed-1",
            "kind": "epub",
            "locator": "{\"href\":\"/ch1\"}",
            "page_number": 12,
            "position_seconds": 34.5,
            "annotations": "[{\"id\":\"a1\"}]",
        }),
    )
    .await;

    expect_echoed(
        &client,
        "list_reader_bookmarks",
        json!({ "edition_id": "ed-1" }),
    )
    .await;

    expect_echoed(
        &client,
        "add_reader_bookmark",
        json!({
            "edition_id": "ed-1",
            "kind": "pdf",
            "locator": "{\"href\":\"/ch1\"}",
            "page_number": 12,
            "position_seconds": 34.5,
            "fraction": 0.42,
        }),
    )
    .await;

    expect_echoed(
        &client,
        "remove_reader_bookmark",
        json!({ "bookmark_id": "bm-1" }),
    )
    .await;

    // Omitted `mode` defaults to `"link"` for both import tools.
    let linked = call(&client, "import_file", json!({ "path": "/books/one.epub" })).await;
    assert_eq!(linked.is_error, Some(false), "import_file must succeed");
    assert_eq!(
        linked.structured_content,
        Some(json!({ "path": "/books/one.epub", "mode": "link" })),
        "import_file must default mode to link"
    );

    let linked_many = call(
        &client,
        "import_files",
        json!({ "paths": ["/books/a.epub", "/books/b.epub"] }),
    )
    .await;
    assert_eq!(
        linked_many.is_error,
        Some(false),
        "import_files must succeed"
    );
    assert_eq!(
        linked_many.structured_content,
        Some(json!({ "paths": ["/books/a.epub", "/books/b.epub"], "mode": "link" })),
        "import_files must default mode to link"
    );

    assert_eq!(
        bridge.calls(),
        vec![
            "search_typeahead",
            "search_editions",
            "filter_options",
            "import_file",
            "import_files",
            "create_virtual_book",
            "reader_publication",
            "reader_resource",
            "get_reader_position",
            "save_reader_position",
            "list_reader_bookmarks",
            "add_reader_bookmark",
            "remove_reader_bookmark",
            "import_file",
            "import_files",
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<String>>()
    );

    endpoint.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_errors_arrive_as_structured_error_content() {
    let endpoint = serve(StubBridge::default(), 0)
        .await
        .expect("bind loopback");
    let client = connect(&endpoint).await;

    let failed = client
        .call_tool(
            CallToolRequestParams::new("remove_reader_bookmark")
                .with_arguments(arguments(json!({ "bookmark_id": "bad" }))),
        )
        .await
        .expect("call remove_reader_bookmark");

    assert_eq!(failed.is_error, Some(true));
    assert_eq!(
        failed.structured_content,
        Some(json!({ "code": "invalid-id", "message": "bookmark id is invalid" }))
    );

    endpoint.shutdown();
}

/// A page in a browser can reach loopback; only the `Host` allowlist stops it
/// from driving the endpoint. Assert the guard is actually on.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn forged_host_header_is_rejected() {
    let endpoint = serve(StubBridge::default(), 0)
        .await
        .expect("bind loopback");

    let mut stream = tokio::net::TcpStream::connect(endpoint.local_addr())
        .await
        .expect("connect to the endpoint");
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"probe","version":"1"}}}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: evil.example\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream
        .write_all(request.as_bytes())
        .await
        .expect("send the request");

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .await
        .expect("read the response");
    assert!(
        response.starts_with("HTTP/1.1 403"),
        "a forged Host header must be refused, got: {response}"
    );

    endpoint.shutdown();
}
