//! One-shot HTTP listener for the loopback OAuth redirect.
//!
//! The Authorization Server redirects the browser to a fixed loopback URI
//! (`http://127.0.0.1:<port>/oauth-redirect`), so the app must be listening
//! before the user approves. One request is served, then the socket closes.

use std::time::Duration;

use crate::error::AtprotoError;

/// How long to wait for the browser to come back.
const ACCEPT_TIMEOUT: Duration = Duration::from_secs(300);
/// A redirect request head is a few hundred bytes; cap it to stay bounded.
const MAX_HEAD: usize = 8 * 1024;

/// Wait for one redirect request and return the callback URL it carried.
pub(crate) async fn await_callback(redirect_uri: &str) -> Result<String, AtprotoError> {
    let expected = url::Url::parse(redirect_uri)
        .map_err(|_| AtprotoError::authorization("invalid loopback redirect URI"))?;
    let port = expected
        .port()
        .ok_or_else(|| AtprotoError::authorization("loopback redirect URI needs a port"))?;

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .map_err(|error| {
            AtprotoError::authorization(format!("could not listen on 127.0.0.1:{port}: {error}"))
        })?;

    let (mut stream, _) = tokio::time::timeout(ACCEPT_TIMEOUT, listener.accept())
        .await
        .map_err(|_| AtprotoError::authorization("timed out waiting for the browser redirect"))?
        .map_err(|error| AtprotoError::authorization(format!("loopback accept failed: {error}")))?;

    let head = tokio::time::timeout(ACCEPT_TIMEOUT, read_head(&mut stream))
        .await
        .map_err(|_| AtprotoError::authorization("timed out reading the browser redirect"))??;

    let target = request_target(&head)
        .ok_or_else(|| AtprotoError::authorization("malformed loopback request"))?;
    let callback = callback_url(port, &target, &expected)?;

    // The browser only needs to know it can be closed; the app reports the
    // outcome in its own UI.
    let body = "<!doctype html><meta charset=\"utf-8\"><title>Livtet</title>\
                <p>Signed in. You can close this tab and return to Livtet.";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    use tokio::io::AsyncWriteExt;
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;

    Ok(callback)
}

async fn read_head(stream: &mut tokio::net::TcpStream) -> Result<String, AtprotoError> {
    use tokio::io::AsyncReadExt;

    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk).await.map_err(|error| {
            AtprotoError::authorization(format!("loopback read failed: {error}"))
        })?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") || buffer.len() >= MAX_HEAD {
            break;
        }
    }

    String::from_utf8(buffer)
        .map_err(|_| AtprotoError::authorization("loopback request was not valid UTF-8"))
}

/// `GET /oauth-redirect?code=…&state=… HTTP/1.1` → `/oauth-redirect?code=…`.
fn request_target(head: &str) -> Option<String> {
    let line = head.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let target = parts.next()?;
    (method == "GET").then(|| target.to_string())
}

/// Rebuild the callback URL the desktop expects, rejecting a wrong path.
fn callback_url(port: u16, target: &str, expected: &url::Url) -> Result<String, AtprotoError> {
    let candidate = url::Url::parse(&format!("http://127.0.0.1:{port}{target}"))
        .map_err(|_| AtprotoError::authorization("malformed loopback callback"))?;
    if candidate.path() != expected.path() {
        return Err(AtprotoError::authorization(
            "loopback callback path did not match the registered redirect URI",
        ));
    }
    Ok(candidate.to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn expected() -> url::Url {
        url::Url::parse("http://127.0.0.1:47821/oauth-redirect").unwrap()
    }

    #[test]
    fn reads_the_target_from_a_get_request() {
        let head = "GET /oauth-redirect?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(
            request_target(head).as_deref(),
            Some("/oauth-redirect?code=abc&state=xyz")
        );
    }

    #[test]
    fn ignores_a_request_that_is_not_a_get() {
        let head = "POST /oauth-redirect HTTP/1.1\r\n\r\n";
        assert!(request_target(head).is_none());
    }

    #[test]
    fn rebuilds_the_callback_url_on_the_loopback_origin() {
        let url = callback_url(47821, "/oauth-redirect?code=abc&state=xyz", &expected()).unwrap();
        assert_eq!(
            url,
            "http://127.0.0.1:47821/oauth-redirect?code=abc&state=xyz"
        );
    }

    #[tokio::test]
    async fn serves_the_callback_and_returns_the_url() {
        use tokio::io::AsyncWriteExt;

        // Grab a free port, release it, then let the listener take it.
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let redirect_uri = format!("http://127.0.0.1:{port}/oauth-redirect");

        let server = tokio::spawn(async move { await_callback(&redirect_uri).await });

        let mut stream = None;
        for _ in 0..50 {
            match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                Ok(connected) => {
                    stream = Some(connected);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
            }
        }
        let mut stream = stream.unwrap();
        stream
            .write_all(
                b"GET /oauth-redirect?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
            )
            .await
            .unwrap();

        let url = server.await.unwrap().unwrap();
        assert_eq!(
            url,
            format!("http://127.0.0.1:{port}/oauth-redirect?code=abc&state=xyz")
        );
    }

    #[test]
    fn rejects_a_different_path() {
        let error = callback_url(47821, "/favicon.ico", &expected()).unwrap_err();
        assert!(error.to_string().contains("did not match"));
    }
}
