//! Application-mediated answers to plugin capability callbacks.
//!
//! Some capabilities are forwarded from the out-of-process host back to the
//! application (a `CallbackCall`) so the app keeps policy and secrets on its
//! side of the boundary. `fs_read` is answered in [`super::importers`]; `http`
//! is answered here so the app can enforce a domain allowlist and (later) attach
//! credentials — the plugin never touches the socket.

use std::io::Read;
use std::time::Duration;

use serde_json::{Value as Json, json};
use stanchion::remote::CallbackCall;

/// Largest response body the `http` capability returns, in bytes.
const MAX_HTTP_BODY_BYTES: u64 = 8 * 1024 * 1024;
/// Per-request timeout for the `http` capability.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// Answers an `http` capability call.
///
/// The plugin passed a request table `{ url, method?, headers?, body? }`. The
/// request is performed only if its host is in `allowlist`; the reply is
/// `{ status, headers, body }`, with the body capped at [`MAX_HTTP_BODY_BYTES`].
pub fn answer_http(call: &CallbackCall, allowlist: &[String]) -> Result<Json, String> {
    if call.capability != "http" {
        return Err(format!("unsupported capability `{}`", call.capability));
    }

    let request = call
        .args
        .first()
        .and_then(Json::as_object)
        .ok_or("http: the request (argument 1) must be a table")?;
    let url_str = request
        .get("url")
        .and_then(Json::as_str)
        .ok_or("http: request.url is required")?;
    let url = url::Url::parse(url_str).map_err(|err| format!("http: invalid url: {err}"))?;
    let host = url.host_str().ok_or("http: url has no host")?;
    if !host_allowed(host, allowlist) {
        return Err(format!("http: host `{host}` is not allowed"));
    }

    let method_str = request.get("method").and_then(Json::as_str).unwrap_or("GET");
    let method = reqwest::Method::from_bytes(method_str.as_bytes())
        .map_err(|err| format!("http: invalid method: {err}"))?;

    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|err| format!("http: {err}"))?;
    let mut builder = client.request(method, url);
    if let Some(headers) = request.get("headers").and_then(Json::as_object) {
        for (name, value) in headers {
            if let Some(value) = value.as_str() {
                builder = builder.header(name, value);
            }
        }
    }
    if let Some(body) = request.get("body").and_then(Json::as_str) {
        builder = builder.body(body.to_string());
    }

    let response = builder.send().map_err(|err| format!("http: {err}"))?;
    let status = response.status().as_u16();
    let mut headers = serde_json::Map::new();
    for (name, value) in response.headers() {
        if let Ok(value) = value.to_str() {
            headers.insert(name.to_string(), Json::from(value));
        }
    }
    let bytes = read_capped(response, MAX_HTTP_BODY_BYTES)?;
    let body = String::from_utf8_lossy(&bytes).into_owned();

    Ok(json!({
        "status": status,
        "headers": Json::Object(headers),
        "body": body,
    }))
}

/// A host matches an allowlist entry exactly or as a subdomain (`.example.com`).
fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist
        .iter()
        .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
}

fn read_capped(response: reqwest::blocking::Response, cap: u64) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    response
        .take(cap.saturating_add(1))
        .read_to_end(&mut buffer)
        .map_err(|err| format!("http: reading response: {err}"))?;
    if buffer.len() as u64 > cap {
        return Err(format!("http: response exceeds {cap} bytes"));
    }
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(args: Vec<Json>) -> CallbackCall {
        CallbackCall {
            plugin: "probe".into(),
            capability: "http".into(),
            grant: Json::Null,
            args,
        }
    }

    #[test]
    fn rejects_a_host_outside_the_allowlist() {
        let allow = vec!["openlibrary.org".to_string()];
        let err = answer_http(&call(vec![json!({ "url": "https://evil.example/x" })]), &allow)
            .expect_err("non-allowlisted host refused");
        assert!(err.contains("not allowed"), "got {err}");
    }

    #[test]
    fn subdomain_matches_an_allowlist_entry() {
        assert!(host_allowed("covers.openlibrary.org", &["openlibrary.org".to_string()]));
        assert!(host_allowed("openlibrary.org", &["openlibrary.org".to_string()]));
        assert!(!host_allowed("notopenlibrary.org", &["openlibrary.org".to_string()]));
    }

    #[test]
    fn fetches_from_an_allowlisted_local_server() {
        // A one-shot HTTP/1.1 server on loopback, no async runtime involved.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Write;
                let mut discard = [0u8; 1024];
                let _ = std::io::Read::read(&mut stream, &mut discard);
                let body = "hello";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        let allow = vec!["127.0.0.1".to_string()];
        let reply = answer_http(
            &call(vec![json!({ "url": format!("http://{addr}/") })]),
            &allow,
        )
        .expect("fetch succeeds");
        assert_eq!(reply["status"], json!(200));
        assert_eq!(reply["body"], json!("hello"));
        handle.join().unwrap();
    }
}
