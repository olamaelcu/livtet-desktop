//! Edition metadata enrichment from plugin enrichers (ADR-0046).
//!
//! Two commands, both keyed on an edition:
//!
//! - [`preview_enrichment`] asks every installed enricher that understands the
//!   edition's identifiers, combines their answers, and reports the diff without
//!   touching the catalog. The combined answer is cached.
//! - [`apply_enrichment`] re-reads the edition, re-merges the cached answer under
//!   the requested mode, writes the result in one transaction, stores the cover
//!   afterwards, and reindexes the edition.
//!
//! Preview and apply call the same [`merge`], so the diff a reader confirms
//! cannot drift from what lands. Bulk enrichment is not implemented.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use camino::Utf8Path;
use serde_json::{Value as Json, json};
use stanchion::remote::{RemoteOptions, RemoteRegistry};
use tauri::State;

use super::enrich_merge::{
    EnrichMode, EnrichmentPreview, EnrichmentSummary, MergeContext, combine, merge,
};
use super::enrich_store::{read_snapshot, write_enrichment};
use super::plugin_host_callbacks::answer_http;
use crate::error::PluginError;
use crate::types::AppState;
use livtet_importer::{BookIdentity, Enrichment};
use livtet_types::DbId;

/// How long a preview stays applicable. A reader who leaves the review screen
/// open longer than this is asked to run Enrich again rather than applying a
/// result fetched against a stale catalog.
const PREVIEW_TTL: Duration = Duration::from_secs(600);

/// Bounds the cache. One entry per edition previewed, oldest evicted first.
const PREVIEW_CAPACITY: usize = 32;

/// The combined answer a preview fetched, waiting for its apply.
///
/// Held server-side rather than round-tripped through the webview: apply must not
/// trust client-supplied metadata, and re-running the lookup would double the
/// network and let the applied result differ from the previewed one.
pub struct CachedEnrichment {
    pub enrichment: Enrichment,
    pub sources: Vec<String>,
    fetched_at: Instant,
}

/// Cache a preview, dropping entries that have aged out and evicting the oldest
/// when full.
fn store_cached(
    cache: &mut HashMap<String, CachedEnrichment>,
    edition_id: String,
    entry: CachedEnrichment,
) {
    cache.retain(|_, cached| cached.fetched_at.elapsed() < PREVIEW_TTL);
    let already_present = cache.contains_key(&edition_id);
    if !already_present
        && cache.len() >= PREVIEW_CAPACITY
        && let Some(oldest) = cache
            .iter()
            .min_by_key(|(_, cached)| cached.fetched_at)
            .map(|(key, _)| key.clone())
    {
        cache.remove(&oldest);
    }
    cache.insert(edition_id, entry);
}

/// Take a cached preview for apply, refusing one that has aged out.
fn take_cached(
    cache: &mut HashMap<String, CachedEnrichment>,
    edition_id: &str,
    ttl: Duration,
) -> Option<CachedEnrichment> {
    let entry = cache.remove(edition_id)?;
    (entry.fetched_at.elapsed() < ttl).then_some(entry)
}

fn parse_edition_id(raw: &str) -> Result<DbId, PluginError> {
    raw.parse::<DbId>()
        .map_err(|_| PluginError::invalid(format!("`{raw}` is not an edition id")))
}

/// Fetch what the installed enrichers know about one edition, then report the
/// diff. Nothing in the catalog is written.
#[tauri::command]
#[specta::specta]
pub async fn preview_enrichment(
    state: State<'_, AppState>,
    edition_id: String,
) -> Result<EnrichmentPreview, PluginError> {
    let id = parse_edition_id(&edition_id)?;
    let snapshot = read_snapshot(&state.db.db_conn(), id)
        .await
        .map_err(PluginError::host)?
        .ok_or_else(|| PluginError::invalid(format!("no edition `{edition_id}`")))?;

    if snapshot.identity.identifiers.is_empty() {
        return Err(PluginError::invalid(
            "this edition carries no identifiers, so no enricher can be matched to it",
        ));
    }

    let answers = enrich_from_plugins(
        &state.plugin_host_path,
        &state.plugin_host_config,
        &state.plugins_dir,
        snapshot.identity.clone(),
    )
    .await?;

    if answers.is_empty() {
        return Err(PluginError::unavailable(
            "no installed enricher understands this edition's identifiers; open /plugins to add one",
        ));
    }

    let (sources, combined) = combine(answers);
    let context = MergeContext {
        sources: sources.clone(),
        cover_storable: snapshot.cover_storable,
    };
    // Any mode classifies identically; `Fill` is the resolution apply would take
    // unless the reader picks another, and the preview never reads `applied`.
    let classified = merge(&snapshot.fields, &combined, EnrichMode::Fill, &context);

    {
        let mut cache = state.enrichment_cache.write().await;
        store_cached(
            &mut cache,
            edition_id,
            CachedEnrichment {
                enrichment: combined,
                sources,
                fetched_at: Instant::now(),
            },
        );
    }

    Ok(classified.preview)
}

/// Apply the previewed enrichment under `mode`.
#[tauri::command]
#[specta::specta]
pub async fn apply_enrichment(
    state: State<'_, AppState>,
    edition_id: String,
    mode: EnrichMode,
) -> Result<EnrichmentSummary, PluginError> {
    let id = parse_edition_id(&edition_id)?;

    let cached = {
        let mut cache = state.enrichment_cache.write().await;
        take_cached(&mut cache, &edition_id, PREVIEW_TTL)
    }
    .ok_or_else(|| PluginError::invalid("preview expired; run Enrich again"))?;

    // Re-read: the catalog may have moved since the preview, and the merge — not
    // the cached preview — decides what is written.
    let snapshot = read_snapshot(&state.db.db_conn(), id)
        .await
        .map_err(PluginError::host)?
        .ok_or_else(|| PluginError::invalid(format!("no edition `{edition_id}`")))?;

    let context = MergeContext {
        sources: cached.sources.clone(),
        cover_storable: snapshot.cover_storable,
    };
    let resolved = merge(&snapshot.fields, &cached.enrichment, mode, &context);

    let outcome = write_enrichment(&state.db, &state.covers_dir, id, &resolved.applied)
        .await
        .map_err(PluginError::host)?;

    let connection = state.db.db_conn();
    if let Some(index) = state.search_index.read().await.as_deref() {
        index
            .add_edition(&connection, id)
            .await
            .map_err(|error| PluginError::host(format!("reindexing the edition: {error}")))?;
    }

    let overwritten = if mode == EnrichMode::Overwrite {
        count(resolved.preview.conflicts.len())
    } else {
        0
    };
    Ok(EnrichmentSummary {
        edition_id,
        sources: cached.sources,
        mode,
        fields_added: count(resolved.preview.added.len()),
        fields_overwritten: overwritten,
        fields_unioned: count(resolved.preview.unioned.len()),
        identifiers_persisted: count(outcome.identifiers_written),
        cover_stored: outcome.cover_path.is_some(),
    })
}

/// Counts cross the IPC boundary as `u32`: Specta will not export `usize`, whose
/// TypeScript analogue is a BigInt with different precision.
fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Bulk enrichment across a filter. Not implemented; the signature carries
/// [`EnrichMode`] so it cannot drift from the single-edition contract when it is.
#[tauri::command]
#[specta::specta]
pub async fn enrich_library(
    _state: State<'_, AppState>,
    _filter: String,
    _mode: EnrichMode,
) -> Result<Vec<EnrichmentSummary>, PluginError> {
    bulk_enrich_unavailable()
}

fn bulk_enrich_unavailable() -> Result<Vec<EnrichmentSummary>, PluginError> {
    Err(PluginError::unavailable(
        "bulk enrich library is planned for a future release",
    ))
}

/// Runs every installed enricher plugin that understands this identity, pairing
/// each answer with the plugin that produced it.
///
/// The host is launched per call on a blocking worker (the registry is not
/// `Send`) and answers only `http`, under the policy in
/// [`super::plugin_host_callbacks::answer_http`]: the allowlist is the app's own
/// read of the plugin's manifest, so nothing here trusts what the host claims.
///
/// `sources` selects the candidates, since an enricher decides which identifier
/// kinds it understands. A plugin that fails `sources` — an importer, say — is
/// skipped, and a plugin whose kinds miss the identity is never asked. No
/// candidate is not an error: the caller decides what to say about an empty
/// result. Every candidate failing `enrich` is, and reports the last failure.
pub(crate) async fn enrich_from_plugins(
    host: &Utf8Path,
    config: &Utf8Path,
    plugins_dir: &Utf8Path,
    identity: BookIdentity,
) -> Result<Vec<(String, Enrichment)>, PluginError> {
    let host = host.to_owned();
    let config = config.to_owned();
    let plugins_dir = plugins_dir.to_owned();
    tokio::task::spawn_blocking(move || enrich_on_host(&host, &config, &plugins_dir, identity))
        .await
        .map_err(|err| PluginError::host(format!("enrich worker failed: {err}")))?
}

/// The blocking body of [`enrich_from_plugins`], run on a worker thread.
fn enrich_on_host(
    host: &Utf8Path,
    config: &Utf8Path,
    plugins_dir: &Utf8Path,
    identity: BookIdentity,
) -> Result<Vec<(String, Enrichment)>, PluginError> {
    let options = RemoteOptions::new(host.as_std_path())
        .config(config)
        .plugins(plugins_dir)
        .inherit_stderr(false);
    let remote = RemoteRegistry::launch(options).map_err(PluginError::host)?;
    let callback_dir = plugins_dir.to_owned();
    let mut remote = remote
        .on_callback(move |call| answer_http(call, &callback_dir))
        .allow_capabilities(["http"]);

    let outcomes = remote.dispatch("sources", []).map_err(PluginError::host)?;
    let mut results = Vec::new();
    let mut last_error: Option<String> = None;
    for outcome in outcomes {
        // A plugin that cannot answer `sources` is not an enricher; its failure
        // is expected, not a reason to abandon the others.
        let Some(value) = outcome.value else {
            continue;
        };
        let Ok(sources) = serde_json::from_value::<Vec<String>>(value) else {
            continue;
        };
        if !enricher_matches_identity(&sources, &identity) {
            continue;
        }

        let plugin = outcome.plugin;
        let answer = remote
            .call::<Json>(&plugin, "enrich", [json!(identity)])
            .map_err(|err| format!("{plugin}: {err}"))
            .and_then(|value| {
                Enrichment::from_wire_json(value).map_err(|err| format!("{plugin}: {err}"))
            });
        match answer {
            Ok(enrichment) => results.push((plugin, enrichment)),
            Err(err) => last_error = Some(err),
        }
    }

    if results.is_empty()
        && let Some(error) = last_error
    {
        return Err(PluginError::host(error));
    }
    Ok(results)
}

/// Whether an enricher's `sources` answers for this identity.
///
/// `sources` names the identifier kinds the plugin keys on (`["isbn", "olid"]`);
/// a plugin that understands none of the identity's kinds is never asked, since
/// it would only come back empty-handed.
fn enricher_matches_identity(sources: &[String], identity: &BookIdentity) -> bool {
    identity.identifiers.iter().any(|identifier| {
        sources
            .iter()
            .any(|kind| kind.trim().eq_ignore_ascii_case(&identifier.kind))
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use camino::Utf8PathBuf;
    use fs_err as fs;

    use super::*;
    use livtet_importer::Identifier;

    fn cached_at(age: Duration) -> CachedEnrichment {
        CachedEnrichment {
            enrichment: Enrichment {
                title: None,
                title_sort: None,
                contributors: Vec::new(),
                publisher: None,
                language: None,
                published: None,
                description: None,
                subjects: Vec::new(),
                cover: None,
                found: Vec::new(),
            },
            sources: vec![String::from("openlibrary")],
            fetched_at: Instant::now() - age,
        }
    }

    #[test]
    fn a_preview_is_available_until_its_ttl_expires() {
        let mut cache = HashMap::new();
        store_cached(&mut cache, String::from("ed-1"), cached_at(Duration::ZERO));

        let fresh = take_cached(&mut cache, "ed-1", PREVIEW_TTL);
        assert!(fresh.is_some(), "a just-fetched preview applies");
        assert!(
            take_cached(&mut cache, "ed-1", PREVIEW_TTL).is_none(),
            "apply consumes the entry"
        );
    }

    #[test]
    fn an_expired_preview_is_refused_and_dropped() {
        let mut cache = HashMap::new();
        store_cached(
            &mut cache,
            String::from("ed-1"),
            cached_at(Duration::from_secs(601)),
        );

        assert!(take_cached(&mut cache, "ed-1", PREVIEW_TTL).is_none());
        assert!(cache.is_empty(), "the stale entry is gone, not left to age");
    }

    #[test]
    fn storing_drops_entries_that_have_already_expired() {
        let mut cache = HashMap::new();
        store_cached(
            &mut cache,
            String::from("stale"),
            cached_at(Duration::from_secs(700)),
        );
        store_cached(&mut cache, String::from("fresh"), cached_at(Duration::ZERO));

        assert_eq!(cache.len(), 1);
        assert!(cache.contains_key("fresh"));
    }

    #[test]
    fn the_cache_evicts_the_oldest_entry_when_full() {
        let mut cache = HashMap::new();
        for index in 0..PREVIEW_CAPACITY {
            // Older first, so eviction has a determinate target.
            let age = Duration::from_secs(u64::try_from(PREVIEW_CAPACITY - index).unwrap());
            store_cached(&mut cache, format!("ed-{index}"), cached_at(age));
        }
        assert_eq!(cache.len(), PREVIEW_CAPACITY);

        let oldest = String::from("ed-0");
        store_cached(
            &mut cache,
            String::from("overflow"),
            cached_at(Duration::ZERO),
        );

        assert_eq!(cache.len(), PREVIEW_CAPACITY, "the cap holds");
        assert!(!cache.contains_key(&oldest), "the oldest entry made room");
        assert!(cache.contains_key("overflow"));
    }

    fn dir(root: &tempfile::TempDir, name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from_path_buf(root.path().join(name)).unwrap()
    }

    /// The host binary cargo builds beside the test binary.
    fn host_binary() -> Utf8PathBuf {
        let test_binary = std::env::current_exe().unwrap();
        let profile_dir = test_binary.parent().and_then(|path| path.parent()).unwrap();
        Utf8PathBuf::from_path_buf(profile_dir.join(if cfg!(windows) {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        }))
        .unwrap()
    }

    /// Forwards `http` callbacks to the app and nothing else. No signatures, so
    /// the fixture plugins load unmodified.
    fn write_host_config(root: &tempfile::TempDir) -> Utf8PathBuf {
        let config = dir(root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"http\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();
        config
    }

    /// Writes a plugin directory; `capabilities` is appended to the manifest as
    /// written, so each test declares exactly the grant it is about.
    fn write_plugin(plugins_dir: &Utf8Path, name: &str, capabilities: &str, lua: &str) {
        let plugin_dir = plugins_dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            format!("name = \"{name}\"\nversion = \"1.0.0\"\n\n{capabilities}"),
        )
        .unwrap();
        fs::write(plugin_dir.join("init.lua"), lua).unwrap();
    }

    fn identity(kind: &str, value: &str) -> BookIdentity {
        BookIdentity {
            identifiers: vec![Identifier {
                kind: kind.to_string(),
                value: value.to_string(),
            }],
            title: None,
            authors: Vec::new(),
        }
    }

    /// A one-shot HTTP/1.1 server on loopback, no async runtime involved.
    fn serve_once(body: &'static str) -> (std::net::SocketAddr, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let _ = stream.read(&mut [0u8; 1024]);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (addr, handle)
    }

    #[test]
    fn sources_select_the_enricher_by_identifier_kind() {
        let isbn = identity("isbn", "9780441013593");

        assert!(enricher_matches_identity(&["isbn".to_string()], &isbn));
        assert!(enricher_matches_identity(
            &["olid".to_string(), " ISBN ".to_string()],
            &isbn
        ));
        assert!(!enricher_matches_identity(&["olid".to_string()], &isbn));
        assert!(!enricher_matches_identity(&[], &isbn));
    }

    /// A bundled-style enricher declares its domains and actually fetches over
    /// `http` through the app, and what it returns is decoded as an `Enrichment`.
    #[tokio::test]
    async fn a_granted_enricher_fetches_and_returns_metadata() {
        let (addr, server) = serve_once("Dune");
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let lua = format!(
            r#"
local Enricher = {{}}
Enricher.__index = Enricher
function Enricher.new(config, deps) return setmetatable({{}}, Enricher) end
function Enricher:sources() return {{ "isbn" }} end
function Enricher:enrich(identity)
  local res = http({{ url = "http://{addr}/" }})
  return {{
    title = res.body,
    contributors = {{}},
    publisher = "Loopback Press",
    subjects = {{}},
    found = {{ {{ kind = "olid", value = "OL1W" }} }},
  }}
end
return Enricher
"#
        );
        write_plugin(
            &plugins_dir,
            "loopback-enricher",
            "[capabilities.http]\ndomains = [\"127.0.0.1\"]\n",
            &lua,
        );
        let config = write_host_config(&root);

        let results = enrich_from_plugins(
            &host_binary(),
            &config,
            &plugins_dir,
            identity("isbn", "9780441013593"),
        )
        .await
        .expect("the enricher answers");

        assert_eq!(results.len(), 1, "one enricher matched: {results:?}");
        let (plugin, enrichment) = &results[0];
        assert_eq!(plugin, "loopback-enricher");
        assert_eq!(enrichment.title.as_deref(), Some("Dune"));
        assert_eq!(enrichment.publisher.as_deref(), Some("Loopback Press"));
        assert_eq!(
            enrichment.found,
            vec![Identifier {
                kind: "olid".to_string(),
                value: "OL1W".to_string(),
            }]
        );
        server.join().unwrap();
    }

    /// An importer has no `sources`, so `dispatch` reports its failure in place.
    /// That is a skip, not an error, and the enricher beside it still answers.
    #[tokio::test]
    async fn a_plugin_without_sources_is_skipped_not_failed() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        write_plugin(
            &plugins_dir,
            "txt-importer",
            "[capabilities.log]\n",
            r#"
local Importer = {}
Importer.__index = Importer
function Importer.new(config, deps) return setmetatable({}, Importer) end
function Importer:extensions() return { "txt" } end
function Importer:read_metadata(path) return { title = "T" } end
return Importer
"#,
        );
        write_plugin(
            &plugins_dir,
            "static-enricher",
            "[capabilities.log]\n",
            r#"
local Enricher = {}
Enricher.__index = Enricher
function Enricher.new(config, deps) return setmetatable({}, Enricher) end
function Enricher:sources() return { "olid" } end
function Enricher:enrich(identity)
  return {
    title = "Static Title",
    contributors = {},
    publisher = "Static Press",
    subjects = {},
    found = {},
  }
end
return Enricher
"#,
        );
        let config = write_host_config(&root);

        let results = enrich_from_plugins(
            &host_binary(),
            &config,
            &plugins_dir,
            identity("olid", "OL1W"),
        )
        .await
        .expect("a plugin failing sources is not an error");

        assert_eq!(results.len(), 1, "only the enricher answers: {results:?}");
        assert_eq!(results[0].0, "static-enricher");
        assert_eq!(results[0].1.publisher.as_deref(), Some("Static Press"));
    }

    /// Declaring `http` without domains grants nothing: the app refuses on its own
    /// read of the manifest, and no connection is attempted.
    #[tokio::test]
    async fn http_without_a_manifest_domain_is_refused_before_any_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let lua = format!(
            r#"
local Enricher = {{}}
Enricher.__index = Enricher
function Enricher.new(config, deps) return setmetatable({{}}, Enricher) end
function Enricher:sources() return {{ "isbn" }} end
function Enricher:enrich(identity)
  local ok, err = pcall(function() return http({{ url = "http://{addr}/" }}) end)
  return {{
    title = tostring(ok),
    contributors = {{}},
    publisher = tostring(err),
    subjects = {{}},
    found = {{}},
  }}
end
return Enricher
"#
        );
        write_plugin(
            &plugins_dir,
            "no-grant-enricher",
            "[capabilities.http]\n",
            &lua,
        );
        let config = write_host_config(&root);

        let results = enrich_from_plugins(
            &host_binary(),
            &config,
            &plugins_dir,
            identity("isbn", "9780441013593"),
        )
        .await
        .expect("the plugin catches its own refusal");

        assert_eq!(results.len(), 1, "got {results:?}");
        let refusal = results[0].1.publisher.as_deref().unwrap_or_default();
        assert!(
            refusal.contains("manifest grants no domains"),
            "the refusal must name the missing grant, got `{refusal}`"
        );
        assert!(
            listener.accept().is_err(),
            "a refused call must not reach the socket"
        );
    }
}
