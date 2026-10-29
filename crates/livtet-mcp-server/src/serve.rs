//! Loopback-only Streamable HTTP transport for the MCP server.
//!
//! The listener binds `127.0.0.1` explicitly — never a wildcard address — so
//! the endpoint is unreachable from another host without an SSH tunnel. The
//! transport's default `allowed_hosts` (loopback names only) rejects a forged
//! `Host` header, which closes the DNS-rebinding hole that a plain loopback
//! bind leaves open to a browser.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use axum::Router;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::{DesktopBridge, tools::LivtetMcpServer};

/// Port the endpoint uses when the caller does not pick one.
pub const DEFAULT_PORT: u16 = 7777;

/// Path the MCP endpoint is mounted at.
pub const ENDPOINT_PATH: &str = "/mcp";

/// A running endpoint: the socket it accepted, plus a shutdown switch.
#[derive(Debug)]
pub struct McpEndpoint {
    addr: SocketAddr,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl McpEndpoint {
    /// The socket the endpoint is listening on.
    pub fn local_addr(&self) -> SocketAddr {
        self.addr
    }

    /// The URL an MCP client connects to.
    pub fn url(&self) -> String {
        format!("http://{}{ENDPOINT_PATH}", self.addr)
    }

    /// Stop accepting sessions and cancel the ones in flight.
    pub fn shutdown(self) {
        self.shutdown.cancel();
        self.task.abort();
    }
}

/// Bind the endpoint to `127.0.0.1:port` and start serving.
///
/// Each session gets its own [`LivtetMcpServer`] wrapping a clone of
/// `bridge`; sessions are tracked by an in-memory manager, which is all a
/// single-process desktop app needs.
pub async fn serve<B: DesktopBridge>(bridge: B, port: u16) -> io::Result<McpEndpoint> {
    let shutdown = CancellationToken::new();
    let config = StreamableHttpServerConfig::default()
        // Simple request/response tools answer with JSON rather than SSE.
        // A tool that emitted a notification would still fall back to SSE,
        // so nothing can be dropped.
        .with_json_response(true)
        .with_cancellation_token(shutdown.clone());

    let service: StreamableHttpService<LivtetMcpServer<B>, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(LivtetMcpServer::new(bridge.clone())),
            Arc::new(LocalSessionManager::default()),
            config,
        );
    let router = Router::new().nest_service(ENDPOINT_PATH, service);

    // Loopback only: a wildcard bind would expose every command to the LAN.
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;

    let graceful = shutdown.clone();
    let task = tokio::spawn(async move {
        let served = axum::serve(listener, router)
            .with_graceful_shutdown(async move { graceful.cancelled().await })
            .await;
        if let Err(error) = served {
            tracing::error!(%error, "MCP endpoint stopped serving");
        }
    });

    Ok(McpEndpoint {
        addr,
        shutdown,
        task,
    })
}
