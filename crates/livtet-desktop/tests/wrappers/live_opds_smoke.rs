//! Live smoke test for the OPDS server tools on the app's own MCP endpoint.
//!
//! Unlike `livtet-mcp-server/tests/endpoint.rs` (a real transport against a stub
//! bridge), this test needs *the desktop app actually running* with its MCP
//! endpoint open: it drives `LivtetMcpServer` over Streamable HTTP and then
//! fetches the OPDS feed the way an e-reader would, so it covers the whole path
//! tool call → bridge → `OpdsServerRuntime` → real listener → real catalog.
//!
//! It is `#[ignore]`d because CI has no app to talk to. Run it against a dev
//! session:
//!
//! ```text
//! LIVTET_MCP_PORT=7777 mise run dev     # in one terminal
//! mise run smoke:mcp                    # in another
//! ```
//!
//! Set `LIVTET_MCP_URL` to point somewhere else than `http://127.0.0.1:7777/mcp`.
//!
//! The test owns the settings it changes: it captures the stored OPDS settings
//! first, works on a port it picks itself, and restores the captured settings
//! before asserting, so a failed run still leaves the user's catalog as it was.
// Test assertions: a panic is the failure signal, so `expect` documents the
// invariant each step relies on.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::{Ipv4Addr, TcpListener};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    transport::streamable_http_client::{
        StreamableHttpClientTransport, StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};

/// Where the app's MCP endpoint lives unless `LIVTET_MCP_URL` says otherwise.
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:7777/mcp";

/// The tools this smoke test exists to exercise.
const OPDS_TOOLS: [&str; 4] = [
    "opds_server_settings_save",
    "opds_server_start",
    "opds_server_status",
    "opds_server_stop",
];

type Client = rmcp::service::RunningService<rmcp::service::RoleClient, ()>;

/// Connect to the running app, or explain how to start it.
async fn connect() -> Client {
    let url = std::env::var("LIVTET_MCP_URL").unwrap_or_else(|_| DEFAULT_ENDPOINT.to_owned());
    // The dev app rebuilds on every source edit and drops its sockets while it
    // restarts, so a single attempt would report "no app" for a server that is
    // merely coming back. Keep trying for a minute before calling it absent.
    let mut last = String::from("no attempt made");
    for _ in 0..30 {
        let transport = StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(url.clone()),
        );
        match ().serve(transport).await {
            Ok(client) => return client,
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!(
        "no MCP endpoint at {url} after 60s ({last}). Start the app with \
         `LIVTET_MCP_PORT=7777 mise run dev` first."
    )
}

/// A tool call's arguments as the wire shape the SDK expects.
fn arguments(value: Value) -> rmcp::model::JsonObject {
    value.as_object().cloned().expect("an object literal")
}

async fn call(client: &Client, name: &str, args: Value) -> CallToolResult {
    client
        .call_tool(CallToolRequestParams::new(name.to_owned()).with_arguments(arguments(args)))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}

/// The `{ code, message }` payload a failing tool carries.
fn error_payload(result: &CallToolResult) -> Result<Value, String> {
    match (result.is_error, result.structured_content.clone()) {
        (Some(true), Some(payload)) => Ok(payload),
        (Some(true), None) => Err(format!("error without a payload: {result:?}")),
        _ => Err(format!("expected a tool error, got {result:?}")),
    }
}

fn success(result: &CallToolResult) -> Result<Value, String> {
    if result.is_error == Some(true) {
        return Err(format!("tool failed: {result:?}"));
    }
    result
        .structured_content
        .clone()
        .ok_or_else(|| format!("no structured content in {result:?}"))
}

/// `{ enabled, port }` from a status payload.
fn settings(status: &Value) -> Result<(bool, u16), String> {
    let enabled = status["settings"]["enabled"]
        .as_bool()
        .ok_or_else(|| format!("no enabled flag in {status}"))?;
    let port = status["settings"]["port"]
        .as_u64()
        .and_then(|port| u16::try_from(port).ok())
        .ok_or_else(|| format!("no usable port in {status}"))?;
    Ok((enabled, port))
}

/// A port nothing is holding, taken by binding and releasing it.
fn free_port() -> u16 {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("reserve a port");
    listener.local_addr().expect("bound address").port()
}

/// A port that is definitely held: the returned listener keeps it.
fn held_port() -> (TcpListener, u16) {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).expect("hold a port");
    let port = listener.local_addr().expect("held address").port();
    (listener, port)
}

/// `GET /` as an e-reader asks for it: the JSON dialect, parsed.
async fn feed(port: u16) -> Result<Value, String> {
    let response = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}/"))
        .header(reqwest::header::ACCEPT, "application/opds+json")
        .send()
        .await
        .map_err(|error| format!("no OPDS feed on {port}: {error}"))?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("feed on {port} answered {}", response.status()));
    }
    response
        .json()
        .await
        .map_err(|error| format!("feed on {port} was not JSON: {error}"))
}

async fn status(client: &Client) -> Result<Value, String> {
    success(&call(client, "opds_server_status", json!({})).await)
}

/// Everything the smoke test asserts; returns on the first failure so the caller
/// can restore the user's settings before reporting it.
async fn drive(client: &Client) -> Result<(), String> {
    let listed = client.list_tools(None).await.map_err(|e| e.to_string())?;
    let names: Vec<&str> = listed.tools.iter().map(|tool| tool.name.as_ref()).collect();
    for tool in OPDS_TOOLS {
        if !names.contains(&tool) {
            return Err(format!("{tool} is not published; got {names:?}"));
        }
    }

    // Pick a port the app can actually bind, giving up after a few tries so a
    // busy machine does not report a product failure.
    let mut chosen = None;
    for _ in 0..3 {
        let port = free_port();
        let saved = call(
            client,
            "opds_server_settings_save",
            json!({ "enabled": true, "port": port }),
        )
        .await;
        match saved {
            ref result if result.is_error != Some(true) => {
                let payload = success(result)?;
                if payload["running"] != json!(true) {
                    return Err(format!("save did not start the listener: {payload}"));
                }
                let url = payload["url"]
                    .as_str()
                    .ok_or_else(|| format!("no url after start: {payload}"))?;
                if !url.ends_with(&format!(":{port}")) {
                    return Err(format!("url {url} does not name the saved port {port}"));
                }
                chosen = Some(port);
                break;
            }
            ref result => {
                // A bind failure is only tolerated as "keep trying another port".
                let payload = error_payload(result)?;
                if payload["code"] != json!("bind") {
                    return Err(format!("unexpected save failure: {payload}"));
                }
            }
        }
    }
    let port = chosen.ok_or_else(|| String::from("could not bind a free port at all"))?;

    // An e-reader can read the catalog: real HTTP, real feed, real library.
    let body = feed(port).await?;
    let items = body["metadata"]["number_of_items"]
        .as_u64()
        .ok_or_else(|| format!("feed has no item count: {body}"))?;

    // A port already in use is refused under its own code, and — the point of the
    // fix this test guards — the working catalog keeps serving.
    let (_held, taken) = held_port();
    let refused = call(
        client,
        "opds_server_settings_save",
        json!({ "enabled": true, "port": taken }),
    )
    .await;
    let refusal = error_payload(&refused)?;
    if refusal["code"] != json!("bind") {
        return Err(format!("taken port did not report `bind`: {refusal}"));
    }
    let after = status(client).await?;
    if after["running"] != json!(true) {
        return Err(format!("a refused save stopped the listener: {after}"));
    }
    if settings(&after)?.1 != port {
        return Err(format!("a refused save moved the port: {after}"));
    }
    feed(port).await?;

    // Stop closes the socket: the same URL stops answering.
    let stopped = success(&call(client, "opds_server_stop", json!({})).await)?;
    if stopped["running"] != json!(false) {
        return Err(format!("stop left the listener running: {stopped}"));
    }
    if feed(port).await.is_ok() {
        return Err(format!("port {port} still serves a feed after stop"));
    }

    println!(
        "OPDS smoke: {items} works published on port {port}, refused save inert, stop closed the socket"
    );
    Ok(())
}

/// Put back whatever the app had before the test touched it.
async fn restore(client: &Client, previous: &Value) -> Result<(), String> {
    let (enabled, port) = settings(previous)?;
    let restored = call(
        client,
        "opds_server_settings_save",
        json!({ "enabled": enabled, "port": port }),
    )
    .await;
    let payload = success(&restored)?;
    if settings(&payload)? != (enabled, port) {
        return Err(format!(
            "restore did not stick: wanted enabled={enabled} port={port}, got {payload}"
        ));
    }
    Ok(())
}

/// Drive the four OPDS tools against the running app, then restore its settings.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs the desktop app running with LIVTET_MCP_PORT set; run `mise run smoke:mcp`"]
async fn opds_tools_drive_a_running_app() {
    let client = connect().await;
    let previous = status(&client).await.expect("status is readable");

    let outcome = drive(&client).await;
    let restored = restore(&client, &previous).await;

    if let Err(error) = outcome {
        panic!("OPDS smoke failed: {error}");
    }
    if let Err(error) = restored {
        panic!("OPDS smoke left the settings changed: {error}");
    }
}
