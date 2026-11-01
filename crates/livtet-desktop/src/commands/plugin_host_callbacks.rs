//! Application-mediated answers to plugin capability callbacks.
//!
//! Some capabilities are forwarded from the out-of-process host back to the
//! application (a `CallbackCall`) so the app keeps policy and secrets on its
//! side of the boundary. `fs_read` is answered in [`super::importers`]; `http`
//! is answered here so the app can enforce a domain allowlist and attach the
//! credentials it holds — the plugin never touches the socket, and never sees a
//! key.
//!
//! The allowlist is the plugin's own grant, not a caller-supplied list: the host
//! forwards the approved parameters of `[capabilities.http]` with every call, so
//! one plugin's manifest cannot widen another's reach. A call whose grant names
//! no domains reaches nothing (ADR-0031).
//!
//! A call is answered only inside the intersection of two reads of that manifest.
//! The grant is an untrusted hint: the host process forwards it, and a compromised
//! host can claim any grant it likes. The trust anchor is this application's own
//! read of `<plugins_dir>/<plugin>/plugin.toml`, so a plugin never reaches past
//! what the app decided it may reach, and the forwarded grant can only narrow
//! that further. Either side naming no domains, or the two naming no common
//! domain, reaches nothing.

use std::io::Read;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use camino::Utf8Path;
use serde_json::{Value as Json, json};
use stanchion::remote::CallbackCall;

/// Largest response body the `http` capability returns, in bytes.
const MAX_HTTP_BODY_BYTES: u64 = 8 * 1024 * 1024;
/// Per-request timeout for the `http` capability.
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

/// The compiled-in credential a plugin may ask the app to attach, and where it is
/// valid: the Google Books volume API, which meters an unkeyed caller by source
/// address and answers 429 once that is spent.
///
/// A plugin names the secret it wants in its manifest (`app_credentials` under
/// `[capabilities.http]`); this module, not the plugin, decides which host and
/// path that secret may travel to. So the app never names a plugin, and a plugin
/// cannot redirect a key somewhere its owner did not intend.
const GOOGLE_BOOKS_SECRET: &str = "google_books";
const GOOGLE_BOOKS_API_HOST: &str = "www.googleapis.com";
const GOOGLE_BOOKS_API_PATH: &str = "/books/v1/";

/// Answers an `http` capability call.
///
/// The plugin passed a request table `{ url, method?, headers?, body? }`. The
/// request is performed only if its host is in the intersection of the manifest
/// on disk ([`granted_domains_for_plugin`]), which is the application's own
/// policy, and the grant the host forwarded with the call, an untrusted hint.
/// The reply is `{ status, headers, body }`, with the body capped at
/// [`MAX_HTTP_BODY_BYTES`].
///
/// An absent or empty list on either side refuses every host rather than falling
/// back to a permissive default: the host is contract-agnostic (ADR-0029), so
/// this check is the only thing standing between a sandboxed plugin and the
/// network.
///
/// A request that survives the check may still gain the app's own credential on
/// the way out ([`attach_credential`]); the plugin is told nothing about it.
pub fn answer_http(call: &CallbackCall, plugins_dir: &Utf8Path) -> Result<Json, String> {
    if call.capability != "http" {
        return Err(format!("unsupported capability `{}`", call.capability));
    }

    let policy = manifest_policy(plugins_dir, &call.plugin);
    let declared = policy.domains;
    if declared.is_empty() {
        return Err("http: the plugin's manifest grants no domains".to_string());
    }
    let forwarded = granted_domains(&call.grant);
    if forwarded.is_empty() {
        return Err("http: the host's grant names no domains".to_string());
    }

    let allowlist: Vec<String> = declared
        .into_iter()
        .filter(|domain| forwarded.contains(domain))
        .collect();
    if allowlist.is_empty() {
        return Err(
            "http: the host's grant and the plugin's manifest name no common domain".to_string(),
        );
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
    let mut url = url::Url::parse(url_str).map_err(|err| format!("http: invalid url: {err}"))?;
    let host = url.host_str().ok_or("http: url has no host")?;
    if !host_allowed(host, &allowlist) {
        return Err(format!("http: host `{host}` is not allowed"));
    }
    attach_credential(&policy.credentials, &mut url);

    let method_str = request
        .get("method")
        .and_then(Json::as_str)
        .unwrap_or("GET");
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
    // `body` is text for JSON and HTML; `body_base64` is the same bytes for
    // anything binary — an image must not cross as lossy UTF-8, or every byte
    // that is not valid UTF-8 becomes U+FFFD and the cover is corrupt before the
    // plugin ever sees it. Encoding here is exact and needs no Lua-side encoder.
    let body = String::from_utf8_lossy(&bytes).into_owned();
    let body_base64 = STANDARD.encode(&bytes);

    // Decoded here, not in the plugin: the `json` capability is gone until
    // stanchion#56 is fixed, and a tree of data is exactly what a plugin can
    // still receive. `null` when the body is not JSON.
    let parsed: Option<Json> = serde_json::from_slice(&bytes).ok();

    Ok(json!({
        "status": status,
        "headers": Json::Object(headers),
        "body": body,
        "body_base64": body_base64,
        "json": parsed,
    }))
}

/// A host matches an allowlist entry exactly or as a subdomain (`.example.com`).
fn host_allowed(host: &str, allowlist: &[String]) -> bool {
    allowlist
        .iter()
        .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
}

/// Adds the app's own credential to a request that may carry one.
///
/// The key is compiled into the application ([`crate::secrets_embed`]) rather
/// than declared by the plugin: a manifest is user-editable and readable by
/// anything that can read the plugin's directory, so a key in `[config]` would be
/// a key handed out. The plugin asks for the lookup; the app adds the credential
/// on the way out, and the reply carries no trace of it.
///
/// A plugin gets a secret only by name, and only for the request the secret is
/// valid on: declaring `google_books` does not put the key on a request to some
/// other host, because the host and path live here. Every other request crosses
/// untouched. A request that already names a `key` keeps its own, so a reader who
/// pastes their own key into the plugin still spends that one.
fn attach_credential(declared: &[String], url: &mut url::Url) {
    if !declared.iter().any(|name| name == GOOGLE_BOOKS_SECRET)
        || url.host_str() != Some(GOOGLE_BOOKS_API_HOST)
        || !url.path().starts_with(GOOGLE_BOOKS_API_PATH)
    {
        return;
    }
    if url.query_pairs().any(|(name, _)| name == "key") {
        return;
    }
    let key = crate::secrets_embed::GOOGLE_BOOKS_API_KEY.trim();
    if key.is_empty() {
        return;
    }
    url.query_pairs_mut().append_pair("key", key);
}

/// The manifest fields this module trusts.
///
/// Only the `http` capability's approved parameters are read: the host validates
/// everything else when it loads the plugin, and a field this module does not
/// understand must not widen what the plugin reaches.
#[derive(serde::Deserialize, Default)]
struct PluginManifest {
    #[serde(default)]
    capabilities: ManifestCapabilities,
}

#[derive(serde::Deserialize, Default)]
struct ManifestCapabilities {
    http: Option<ManifestHttp>,
}

#[derive(serde::Deserialize, Default)]
struct ManifestHttp {
    /// Untyped on purpose: an entry that is not a string must narrow access, not
    /// fail the whole manifest open.
    #[serde(default)]
    domains: Vec<toml::Value>,
    /// App-held secrets this plugin asks to have attached, by name. A claim, not
    /// a grant: [`attach_credential`] decides where each named secret may go.
    #[serde(default)]
    app_credentials: Vec<toml::Value>,
}

/// What a plugin's manifest asks for: hosts it may reach, secrets it wants.
#[derive(Debug, Default)]
struct ManifestPolicy {
    domains: Vec<String>,
    credentials: Vec<String>,
}

#[cfg(test)]
#[cfg_attr(test, allow(dead_code))]
/// The domains this plugin's own manifest grants `http`, read from
/// `<plugins_dir>/<plugin>/plugin.toml`.
///
/// This is the application's side of the policy in [`answer_http`]. Every failure
/// — an unsafe plugin name, an unreadable or unparsable manifest, no declared
/// `http` capability — narrows to nothing rather than falling back to a default.
fn granted_domains_for_plugin(plugins_dir: &Utf8Path, plugin: &str) -> Vec<String> {
    manifest_policy(plugins_dir, plugin).domains
}

/// The manifest policy in one read: both sides of it come from the same table,
/// so reading it twice could see two different files.
fn manifest_policy(plugins_dir: &Utf8Path, plugin: &str) -> ManifestPolicy {
    if !is_safe_plugin_name(plugin) {
        return ManifestPolicy::default();
    }
    let manifest_path = plugins_dir.join(plugin).join("plugin.toml");
    let Ok(text) = std::fs::read_to_string(&manifest_path) else {
        return ManifestPolicy::default();
    };
    let Ok(manifest) = toml::from_str::<PluginManifest>(&text) else {
        return ManifestPolicy::default();
    };
    let declared = manifest.capabilities.http.unwrap_or_default();
    ManifestPolicy {
        domains: clean_domains(declared.domains.iter().filter_map(toml::Value::as_str)),
        credentials: declared
            .app_credentials
            .iter()
            .filter_map(toml::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    }
}

/// A plugin name is a single path segment: anything else could read a manifest
/// outside the plugins root.
fn is_safe_plugin_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains('/') && !name.contains('\\')
}

/// The domains this call was granted, ignoring anything the manifest put beside
/// them.
///
/// The host forwards the capability's approved parameters verbatim, so the shape
/// is the manifest's: `{ domains = ["example.org"] }`.
fn granted_domains(grant: &Json) -> Vec<String> {
    let entries = grant
        .get("domains")
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    clean_domains(entries.iter().filter_map(Json::as_str))
}

/// Normalizes a domain list from either side of the policy.
///
/// Blank entries are dropped rather than coerced — a typo in a manifest must
/// narrow access, never widen it.
fn clean_domains<'a>(domains: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    domains
        .into_iter()
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
        .map(str::to_owned)
        .collect()
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use fs_err as fs;

    use super::*;

    fn call(args: Vec<Json>) -> CallbackCall {
        CallbackCall {
            plugin: "probe".into(),
            capability: "http".into(),
            grant: json!({ "domains": ["openlibrary.org"] }),
            args,
        }
    }

    /// Writes `<plugins_dir>/<plugin>/plugin.toml` declaring `http` with
    /// `domains`, which is the application's own policy read.
    fn write_manifest(plugins_dir: &Utf8Path, plugin: &str, domains: &[&str]) {
        let plugin_dir = plugins_dir.join(plugin);
        fs::create_dir_all(&plugin_dir).unwrap();
        let listed: Vec<String> = domains
            .iter()
            .map(|domain| format!("\"{domain}\""))
            .collect();
        fs::write(
            plugin_dir.join("plugin.toml"),
            format!(
                "name = \"{plugin}\"\nversion = \"1.0.0\"\n\n[capabilities.http]\ndomains = [{}]\n",
                listed.join(", ")
            ),
        )
        .unwrap();
    }

    /// A plugins root holding one `probe` plugin whose manifest grants `domains`.
    fn probe_root(domains: &[&str]) -> (tempfile::TempDir, camino::Utf8PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let plugins = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf())
            .unwrap()
            .join("plugins");
        write_manifest(&plugins, "probe", domains);
        (root, plugins)
    }

    #[test]
    fn rejects_a_host_outside_the_grant() {
        let (_root, plugins) = probe_root(&["openlibrary.org"]);
        let err = answer_http(
            &call(vec![json!({ "url": "https://evil.example/x" })]),
            &plugins,
        )
        .expect_err("non-granted host refused");
        assert!(err.contains("not allowed"), "got {err}");
    }

    /// The host is contract-agnostic (ADR-0029), so an empty grant must refuse
    /// everything: there is no permissive default to fall back to.
    #[test]
    fn refuses_every_host_when_the_grant_names_no_domains() {
        let (_root, plugins) = probe_root(&["openlibrary.org"]);
        for grant in [
            Json::Null,
            json!({}),
            json!({ "domains": [] }),
            json!({ "domains": 42 }),
            json!({ "domains": [""] }),
        ] {
            let mut request = call(vec![json!({ "url": "http://127.0.0.1:9/" })]);
            request.grant = grant.clone();
            let error = answer_http(&request, &plugins).expect_err("no grant means no reach");
            assert!(
                error.contains("host's grant names no domains"),
                "grant {grant}: {error}"
            );
        }
    }

    #[test]
    fn a_grant_entry_is_matched_against_the_requested_host_only() {
        let (_root, plugins) = probe_root(&["openlibrary.org"]);
        let mut request = call(vec![json!({ "url": "http://127.0.0.1:9/" })]);
        request.grant = json!({ "domains": ["openlibrary.org"] });

        let error = answer_http(&request, &plugins).expect_err("the grant names another host");
        assert!(error.contains("127.0.0.1"), "{error}");
    }

    /// A manifest typo must narrow access, never widen it.
    #[test]
    fn ignores_non_string_and_blank_grant_entries() {
        let grant = json!({ "domains": [42, null, "  ", " openlibrary.org "] });
        assert_eq!(granted_domains(&grant), vec!["openlibrary.org".to_string()]);
        assert!(granted_domains(&json!({ "domains": "openlibrary.org" })).is_empty());
        assert!(granted_domains(&Json::Null).is_empty());
    }

    #[test]
    fn rejects_a_capability_it_does_not_answer() {
        let (_root, plugins) = probe_root(&["openlibrary.org"]);
        let mut request = call(vec![json!({ "url": "http://127.0.0.1:9/" })]);
        request.capability = "sqlite".into();

        let error = answer_http(&request, &plugins).expect_err("only http is answered here");
        assert!(error.contains("unsupported capability"), "{error}");
    }

    #[test]
    fn subdomain_matches_an_allowlist_entry() {
        assert!(host_allowed(
            "covers.openlibrary.org",
            &["openlibrary.org".to_string()]
        ));
        assert!(host_allowed(
            "openlibrary.org",
            &["openlibrary.org".to_string()]
        ));
        assert!(!host_allowed(
            "notopenlibrary.org",
            &["openlibrary.org".to_string()]
        ));
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

        let mut request = call(vec![json!({ "url": format!("http://{addr}/") })]);
        request.grant = json!({ "domains": ["127.0.0.1"] });
        let (_root, plugins) = probe_root(&["127.0.0.1"]);
        let reply = answer_http(&request, &plugins).expect("fetch succeeds");
        assert_eq!(reply["status"], json!(200));
        assert_eq!(reply["body"], json!("hello"));
        // The same bytes, exact, for consumers that cannot use the lossy text.
        assert_eq!(reply["body_base64"], json!("aGVsbG8="));
        // "hello" is not JSON, so the decoded tree is null rather than absent.
        assert_eq!(reply["json"], Json::Null);
        handle.join().unwrap();
    }

    /// The forwarded grant is the host's word; the manifest is the app's. A
    /// plugin whose manifest grants nothing is refused even when the call
    /// carries domains.
    #[test]
    fn refuses_when_the_manifest_grants_no_domains() {
        let (_root, plugins) = probe_root(&[]);
        let mut request = call(vec![json!({ "url": "http://127.0.0.1:9/" })]);
        request.grant = json!({ "domains": ["127.0.0.1"] });

        let error = answer_http(&request, &plugins).expect_err("the manifest grants nothing");
        assert!(error.contains("manifest grants no domains"), "{error}");
    }

    /// Both sides must name the same domain: a host that narrows to something
    /// the app never granted reaches nothing either.
    #[test]
    fn refuses_when_the_two_sides_name_no_common_domain() {
        let (_root, plugins) = probe_root(&["openlibrary.org"]);
        let mut request = call(vec![json!({ "url": "https://covers.openlibrary.org/x" })]);
        request.grant = json!({ "domains": ["covers.openlibrary.org"] });

        let error = answer_http(&request, &plugins).expect_err("the sides are disjoint");
        assert!(error.contains("name no common domain"), "{error}");
    }

    /// Manifest domains are normalized the same way as the grant's: a non-string
    /// or blank entry narrows, never widens and never fails the whole read open.
    #[test]
    fn normalizes_manifest_domains_like_the_grant() {
        let (_root, plugins) = probe_root(&[]);
        let plugin_dir = plugins.join("probe");
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"probe\"\nversion = \"1.0.0\"\n\n[capabilities.http]\ndomains = [42, \"  \", \" openlibrary.org \"]\n",
        )
        .unwrap();

        assert_eq!(
            granted_domains_for_plugin(&plugins, "probe"),
            vec!["openlibrary.org".to_string()]
        );
    }

    /// The `key` a URL carries, for asserting what a request ended up asking for.
    fn key_in(url: &url::Url) -> Option<String> {
        url.query_pairs()
            .find(|(name, _)| name == "key")
            .map(|(_, value)| value.into_owned())
    }

    /// The compiled-in Google key reaches the Google Books volume API this app's
    /// plugin calls, and nothing else. Every other request — another plugin, another
    /// host, another Google API — must cross without it, and a key the plugin named
    /// itself must survive. This is the whole exposure surface of an embedded
    /// secret, so each arm is asserted rather than implied by a skip.
    #[test]
    fn attaches_the_embedded_key_only_to_the_requests_it_is_valid_on() {
        let embedded = crate::secrets_embed::GOOGLE_BOOKS_API_KEY;
        let parse = |text: &str| url::Url::parse(text).unwrap();
        let declaring = [String::from(GOOGLE_BOOKS_SECRET)];

        let mut books = parse("https://www.googleapis.com/books/v1/volumes?q=isbn%3A9780441013593");
        attach_credential(&declaring, &mut books);
        assert_eq!(key_in(&books), Some(embedded.to_string()));
        assert!(
            books
                .query_pairs()
                .any(|(name, value)| name == "q" && value == "isbn:9780441013593"),
            "attaching the key must not disturb the query it is added to"
        );

        // A plugin that declares nothing gets nothing, whatever it is called.
        let mut undeclared = parse("https://www.googleapis.com/books/v1/volumes");
        attach_credential(&[], &mut undeclared);
        assert_eq!(
            key_in(&undeclared),
            None,
            "a plugin that declares no secret spends none"
        );

        // A declaration of some other secret is not this key.
        let mut other_secret = parse("https://www.googleapis.com/books/v1/volumes");
        attach_credential(&[String::from("sentry_dsn")], &mut other_secret);
        assert_eq!(
            key_in(&other_secret),
            None,
            "only the secret it named is attached"
        );

        let mut another_api = parse("https://www.googleapis.com/customsearch/v1");
        attach_credential(&declaring, &mut another_api);
        assert_eq!(
            key_in(&another_api),
            None,
            "the key is not a Google-wide key"
        );

        let mut another_host = parse("https://books.google.com/books/content?id=x");
        attach_credential(&declaring, &mut another_host);
        assert_eq!(
            key_in(&another_host),
            None,
            "the key never leaves Google's API"
        );

        let mut own_key = parse("https://www.googleapis.com/books/v1/volumes?key=mine");
        attach_credential(&declaring, &mut own_key);
        assert_eq!(key_in(&own_key), Some("mine".to_string()));
    }

    /// The declaration is the plugin's, the scope is the app's: a manifest can
    /// ask for `google_books` but cannot move the key off Google's Books API.
    #[test]
    fn the_manifest_declares_which_secrets_a_plugin_may_ask_for() {
        let root = tempfile::tempdir().unwrap();
        let plugins = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf())
            .unwrap()
            .join("plugins");
        let plugin_dir = plugins.join("probe");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"probe\"\nversion = \"1.0.0\"\n\n[capabilities.http]\ndomains = [\"www.googleapis.com\"]\napp_credentials = [\"google_books\", 7, \"  \"]\n",
        )
        .unwrap();

        let policy = manifest_policy(&plugins, "probe");
        assert_eq!(policy.domains, vec!["www.googleapis.com".to_string()]);
        assert_eq!(
            policy.credentials,
            vec![String::from("google_books")],
            "non-string and blank entries are dropped, like the domain list"
        );
    }

    /// A plugin name is a path segment chosen by the child. Traversal must read
    /// nothing, even when the target directory really holds a `plugin.toml`.
    #[test]
    fn unsafe_plugin_names_read_no_manifest() {
        let root = tempfile::tempdir().unwrap();
        let plugins = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf())
            .unwrap()
            .join("plugins");
        write_manifest(&plugins, "probe", &["127.0.0.1"]);
        // `../x` resolves to this directory if the join is taken at face value.
        let outside = camino::Utf8PathBuf::from_path_buf(root.path().to_path_buf())
            .unwrap()
            .join("x");
        fs::create_dir_all(&outside).unwrap();
        fs::write(
            outside.join("plugin.toml"),
            "name = \"x\"\nversion = \"1.0.0\"\n\n[capabilities.http]\ndomains = [\"127.0.0.1\"]\n",
        )
        .unwrap();

        for unsafe_name in ["../x", "..", ".", "nested/child", ""] {
            assert!(
                granted_domains_for_plugin(&plugins, unsafe_name).is_empty(),
                "{unsafe_name:?} must grant nothing"
            );
        }

        let mut request = call(vec![json!({ "url": "http://127.0.0.1:9/" })]);
        request.plugin = "../x".into();
        request.grant = json!({ "domains": ["127.0.0.1"] });
        let error = answer_http(&request, &plugins).expect_err("traversal is refused");
        assert!(error.contains("manifest grants no domains"), "{error}");
    }
}
