//! Localhost-only Model Context Protocol endpoint for the Livtet desktop app.
//!
//! The endpoint speaks MCP over Streamable HTTP on `127.0.0.1`, exposing a
//! curated subset of the desktop commands (search, import, reader) to local
//! clients. It is a library, not a separate process: `livtet-desktop`
//! implements [`DesktopBridge`] over its live `AppState`, so a tool call runs
//! exactly the code the webview reaches over IPC. No second process, no socket
//! bridge, and no second database connection.
//!
//! Authorization is the bind address: `127.0.0.1` only, with the transport's
//! loopback `Host` allowlist. There is no token; a remote caller needs an SSH
//! tunnel, which is already an authenticated channel.

mod bridge;
mod serve;
mod tools;

pub use bridge::{BridgeError, DesktopBridge};
pub use serve::{DEFAULT_PORT, ENDPOINT_PATH, McpEndpoint, serve};
pub use tools::LivtetMcpServer;
