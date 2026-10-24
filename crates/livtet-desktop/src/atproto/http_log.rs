//! Logging wrapper around the XRPC HTTP client.
//!
//! `atrium-oauth` swallows failures — its `callback` turns a token-exchange
//! error into `todo!()` — so the only way to see why an exchange failed is to
//! log the transport. Secrets never reach the log: successful token responses
//! are reduced to their non-secret fields, and only error bodies are shown
//! verbatim.

use atrium_xrpc::http::{Request, Response};
use atrium_xrpc_client::reqwest::{ReqwestClient, ReqwestClientBuilder};

/// Longest body excerpt written to the log.
const MAX_BODY: usize = 512;

#[derive(Clone)]
pub(crate) struct LoggingHttpClient {
    inner: ReqwestClient,
}

impl LoggingHttpClient {
    pub(crate) fn new(base_uri: &str, client: reqwest::Client) -> Self {
        Self {
            inner: ReqwestClientBuilder::new(base_uri).client(client).build(),
        }
    }
}

impl atrium_xrpc::HttpClient for LoggingHttpClient {
    async fn send_http(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Vec<u8>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let method = request.method().clone();
        let uri = request.uri().to_string();
        let content_length = request.body().len();
        // Presence only: the token and proof values must never be logged.
        let auth_scheme = request
            .headers()
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .map(|value| {
                value
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            });
        let has_dpop = request.headers().contains_key("dpop");

        match self.inner.send_http(request).await {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    tracing::debug!(
                        %method,
                        %uri,
                        %status,
                        content_length,
                        auth_scheme = auth_scheme.as_deref(),
                        has_dpop,
                        "atrium http"
                    );
                } else {
                    let summary = summarize(&uri, response.body());
                    tracing::warn!(
                        %method,
                        %uri,
                        %status,
                        content_length,
                        auth_scheme = auth_scheme.as_deref(),
                        has_dpop,
                        body = %summary,
                        "atrium http rejected"
                    );
                }
                Ok(response)
            }
            Err(error) => {
                tracing::warn!(%method, %uri, content_length, %error, "atrium http failed");
                Err(error)
            }
        }
    }
}

/// Reduce a response body to something safe to log.
fn summarize(uri: &str, body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    if uri.contains("/oauth/token")
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
    {
        // The token endpoint's success payload carries bearer tokens; a failure
        // carries only `error`/`error_description`. Report which is which
        // without ever echoing a token.
        let safe = serde_json::json!({
            "error": value.get("error"),
            "error_description": value.get("error_description"),
            "token_type": value.get("token_type"),
            "scope": value.get("scope"),
            "expires_in": value.get("expires_in"),
            "has_access_token": value.get("access_token").is_some(),
            "has_refresh_token": value.get("refresh_token").is_some(),
        });
        return safe.to_string();
    }
    text.chars().take(MAX_BODY).collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn token_responses_are_reduced_to_non_secret_fields() {
        let body = br#"{"access_token":"secret-access","refresh_token":"secret-refresh","token_type":"DPoP","scope":"atproto","expires_in":3600,"sub":"did:plc:x"}"#;
        let summary = summarize("https://pds.example/oauth/token", body);
        assert!(!summary.contains("secret-access"));
        assert!(!summary.contains("secret-refresh"));
        assert!(summary.contains("\"has_access_token\":true"));
        assert!(summary.contains("\"token_type\":\"DPoP\""));
    }

    #[test]
    fn token_errors_keep_the_description() {
        let body = br#"{"error":"invalid_grant","error_description":"code expired"}"#;
        let summary = summarize("https://pds.example/oauth/token", body);
        assert!(summary.contains("invalid_grant"));
        assert!(summary.contains("code expired"));
    }

    /// Serve one canned response on a loopback port and return its base URI.
    async fn one_shot_server(status: &'static str, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 2048];
            let _ = socket.read(&mut buffer).await;
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn get(base_uri: &str) -> Response<Vec<u8>> {
        let client = LoggingHttpClient::new(base_uri, reqwest::Client::new());
        let request = Request::builder()
            .method(atrium_xrpc::http::Method::GET)
            .uri(format!("{base_uri}/xrpc/com.example.test"))
            .body(Vec::new())
            .unwrap();
        atrium_xrpc::HttpClient::send_http(&client, request)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn returns_successful_responses_unchanged() {
        let base = one_shot_server("200 OK", r#"{"ok":true}"#).await;
        let response = get(&base).await;
        assert_eq!(response.status(), atrium_xrpc::http::StatusCode::OK);
        assert_eq!(response.body().as_slice(), br#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn returns_error_responses_unchanged() {
        let base = one_shot_server("400 Bad Request", r#"{"error":"invalid_grant"}"#).await;
        let response = get(&base).await;
        assert_eq!(
            response.status(),
            atrium_xrpc::http::StatusCode::BAD_REQUEST
        );
        assert_eq!(response.body().as_slice(), br#"{"error":"invalid_grant"}"#);
    }

    #[test]
    fn other_bodies_are_truncated() {
        let long = "x".repeat(MAX_BODY * 2);
        assert_eq!(
            summarize("https://pds.example/xrpc/x", long.as_bytes())
                .chars()
                .count(),
            MAX_BODY
        );
    }
}
