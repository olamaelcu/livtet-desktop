#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Command-wrapper coverage for `commands::{plugins,sync,catalog}`.
//!
//! Each `#[tauri::command]` is called the way Tauri calls it — through
//! `AppState` on a `tauri::test::mock_app()` — so the wrapper body (state
//! cloning, `spawn_blocking` dispatch, daemon error mapping, id parsing) runs.
//! Where a wrapper is a thin delegation, what is asserted here is what the
//! wrapper does with real state: directories really move, the daemon's failure
//! really surfaces, ids really are rejected before a query.

use std::fmt::Debug;

use camino::{Utf8Path, Utf8PathBuf};
use livtet_core::data::entities::{
    authors, digital_inventory, edition_authors, edition_identifiers, edition_publishers, editions,
    formats, identifiers, languages, publishers, works,
};
use livtet_core::data::orm::{ActiveModelTrait, Set};
use livtet_core::data::{Kind, TestDb};
use livtet_desktop_lib::commands::{catalog, plugins, sync};
use livtet_desktop_lib::{AppState, mock_state_for_bulk};
use livtet_types::{DbId, now_primitive};
use serde::Serialize;
use serde_json::Value;
use tauri::Manager;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Build the mock app around a state and hand it back.
fn mock_app(state: AppState) -> tauri::App<tauri::test::MockRuntime> {
    let app = tauri::test::mock_app();
    app.manage(state);
    app
}

/// Temp dir as UTF-8, for the camino-typed plugin roots.
fn utf8(temp: &TempDir) -> Utf8PathBuf {
    Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).expect("utf-8 temp dir")
}

/// The command error types are not re-exported from the crate root, so their
/// behaviour is inspected through the shape the webview receives: an externally
/// tagged enum such as `{ "Unavailable": { "message": "…" } }`.
fn error_of<T, E>(result: Result<T, E>) -> (String, Value)
where
    T: Debug,
    E: Serialize,
{
    let error = match result {
        Ok(value) => panic!("expected the command to fail, got {value:?}"),
        Err(error) => error,
    };
    let value = serde_json::to_value(&error).expect("command errors serialise");
    value
        .as_object()
        .and_then(|object| object.iter().next())
        .map(|(tag, body)| (tag.clone(), body.clone()))
        .expect("externally tagged error")
}

/// Assert the command reported a dependency that is not there (a dead daemon, a
/// missing registry) rather than a protocol or validation failure.
fn assert_unavailable<T, E>(label: &str, result: Result<T, E>)
where
    T: Debug,
    E: Serialize,
{
    let (tag, body) = error_of(result);
    assert_eq!(tag, "Unavailable", "{label}: {body}");
    assert!(
        !body["message"].as_str().unwrap_or_default().is_empty(),
        "{label}: unavailable error carried no message: {body}"
    );
}

// ---------------------------------------------------------------------------
// plugins.rs
// ---------------------------------------------------------------------------

/// A Lua importer the plugin host can actually load.
const LOADABLE_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer
function Importer.new(config, deps) return setmetatable({}, Importer) end
function Importer:extensions() return { "txt" } end
function Importer:read_metadata(path) return { title = "T" } end
return Importer
"#;

/// Write a plugin directory whose manifest the host parses and whose `init.lua`
/// it loads. `plugin_dir` is the directory itself, not its parent.
fn write_plugin(plugin_dir: &Utf8Path, name: &str) {
    fs_err::create_dir_all(plugin_dir).expect("create plugin dir");
    fs_err::write(
        plugin_dir.join("plugin.toml"),
        format!("name = \"{name}\"\nversion = \"1.0.0\"\n\n[capabilities.log]\n"),
    )
    .expect("write manifest");
    fs_err::write(plugin_dir.join("init.lua"), LOADABLE_IMPORTER).expect("write init.lua");
}

/// The same plugin, placed inside a plugin root under its manifest name.
fn write_plugin_in(root: &Utf8Path, name: &str) {
    write_plugin(&root.join(name), name);
}

/// A host config that grants `log`, so a loadable plugin reports that grant.
fn write_host_config(root: &Utf8Path) -> Utf8PathBuf {
    let config = root.join("host.toml");
    fs_err::write(
        &config,
        "[capabilities]\nallow = [\"log\"]\n\n[signatures]\nrequired = false\n",
    )
    .expect("write host config");
    config
}

/// The plugin host built next to the test binaries. The repo's `mise run test`
/// builds every workspace binary; a narrower `-p livtet-desktop` run may not, so
/// host-backed assertions check for it first.
fn host_binary() -> Utf8PathBuf {
    let test_binary = std::env::current_exe().expect("current exe");
    let profile_dir = test_binary
        .parent()
        .and_then(|path| path.parent())
        .expect("target profile dir");
    Utf8PathBuf::from_path_buf(profile_dir.join("livtet-plugin-host")).expect("utf-8 host path")
}

/// A state whose two plugin roots and host live under `root`.
async fn plugin_state(test_db: &TestDb, root: &Utf8Path, host: Utf8PathBuf) -> AppState {
    let mut state = mock_state_for_bulk(test_db).await;
    state.plugins_dir = root.join("plugins");
    state.disabled_plugins_dir = root.join("plugins-disabled");
    state.plugin_host_path = host;
    state.plugin_host_config = write_host_config(root);
    state
}

#[tokio::test]
async fn set_plugin_enabled_moves_the_directory_between_roots() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    write_plugin_in(&root.join("plugins"), "movable");
    let app = mock_app(plugin_state(&test_db, &root, root.join("no-host")).await);

    plugins::set_plugin_enabled(app.state::<AppState>(), "movable".to_string(), false)
        .await
        .expect("disabling an installed plugin succeeds");
    assert!(
        !root.join("plugins").join("movable").exists(),
        "disabled plugin stayed in the root the host scans"
    );
    assert!(
        root.join("plugins-disabled")
            .join("movable/plugin.toml")
            .is_file(),
        "the plugin's files did not move intact"
    );

    plugins::set_plugin_enabled(app.state::<AppState>(), "movable".to_string(), true)
        .await
        .expect("enabling a disabled plugin succeeds");
    assert!(root.join("plugins").join("movable/plugin.toml").is_file());
    assert!(!root.join("plugins-disabled").join("movable").exists());

    // Idempotent: toggling a switch twice is not an error.
    plugins::set_plugin_enabled(app.state::<AppState>(), "movable".to_string(), true)
        .await
        .expect("enabling an enabled plugin succeeds");

    let (tag, body) = error_of(
        plugins::set_plugin_enabled(app.state::<AppState>(), "absent".to_string(), false).await,
    );
    assert_eq!(tag, "Invalid", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("absent"),
        "error did not name the plugin: {body}"
    );
}

#[tokio::test]
async fn set_plugin_enabled_rejects_names_that_escape_a_root() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    fs_err::create_dir_all(root.join("plugins")).expect("plugins root");
    fs_err::create_dir_all(root.join("plugins-disabled")).expect("disabled root");
    let sentinel = root.join("keep.txt");
    fs_err::write(&sentinel, b"keep").expect("sentinel");
    let app = mock_app(plugin_state(&test_db, &root, root.join("no-host")).await);

    for evil in ["..", "../keep.txt", "nested/child", "back\\slash"] {
        for enabled in [true, false] {
            let (tag, body) = error_of(
                plugins::set_plugin_enabled(app.state::<AppState>(), evil.to_string(), enabled)
                    .await,
            );
            assert_eq!(tag, "Invalid", "{evil:?} (enabled={enabled}): {body}");
        }
    }
    assert!(
        sentinel.is_file(),
        "a name escaped the roots and hit a file"
    );
    assert!(
        !root.join("nested").exists(),
        "a traversal created a directory"
    );
}

#[tokio::test]
async fn remove_plugin_deletes_from_whichever_root_holds_it() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    write_plugin_in(&root.join("plugins"), "enabled-one");
    write_plugin_in(&root.join("plugins-disabled"), "disabled-one");
    let app = mock_app(plugin_state(&test_db, &root, root.join("no-host")).await);

    plugins::remove_plugin(app.state::<AppState>(), "enabled-one".to_string())
        .await
        .expect("removing from the scanned root succeeds");
    assert!(!root.join("plugins").join("enabled-one").exists());

    plugins::remove_plugin(app.state::<AppState>(), "disabled-one".to_string())
        .await
        .expect("removing from the disabled root succeeds");
    assert!(
        !root.join("plugins-disabled").join("disabled-one").exists(),
        "uninstalling a disabled plugin silently did nothing"
    );

    let (tag, body) =
        error_of(plugins::remove_plugin(app.state::<AppState>(), "never".to_string()).await);
    assert_eq!(tag, "Invalid", "{body}");
}

/// ADR-0034: the loaded set is exactly the directories in the scanned root, so a
/// disabled plugin is listed (with its real metadata) and marked `enabled: false`.
#[tokio::test]
async fn list_plugins_reports_the_root_a_plugin_sits_in() {
    let host = host_binary();
    if !host.is_file() {
        eprintln!("skipping host-backed assertions: {} is not built", host);
        return;
    }
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    write_plugin_in(&root.join("plugins"), "stays-on");
    write_plugin_in(&root.join("plugins"), "turned-off");
    let app = mock_app(plugin_state(&test_db, &root, host).await);

    plugins::set_plugin_enabled(app.state::<AppState>(), "turned-off".to_string(), false)
        .await
        .expect("disable succeeds");

    let listed = plugins::list_plugins(app.state::<AppState>())
        .await
        .expect("both roots are listed");

    let on = listed
        .iter()
        .find(|plugin| plugin.name == "stays-on")
        .unwrap_or_else(|| panic!("enabled plugin missing from {listed:?}"));
    assert!(
        on.enabled,
        "plugin in the scanned root reported as disabled"
    );
    assert_eq!(on.version.as_deref(), Some("1.0.0"));
    assert!(
        on.granted.iter().any(|grant| grant == "log"),
        "enabled plugin lost its grant: {on:?}"
    );

    let off = listed
        .iter()
        .find(|plugin| plugin.name == "turned-off")
        .unwrap_or_else(|| panic!("disabled plugin missing from {listed:?}"));
    assert!(
        !off.enabled,
        "plugin in the disabled root reported as enabled"
    );
    assert_eq!(
        off.version.as_deref(),
        Some("1.0.0"),
        "metadata did not survive disabling: {off:?}"
    );
}

/// A host that cannot be launched is a host failure, not an empty listing.
#[tokio::test]
async fn list_plugins_maps_a_missing_host_to_a_host_error() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    fs_err::create_dir_all(root.join("plugins")).expect("plugins root");
    let app = mock_app(plugin_state(&test_db, &root, root.join("no-host")).await);

    let (tag, body) = error_of(plugins::list_plugins(app.state::<AppState>()).await);
    assert_eq!(tag, "Host", "an empty listing was reported instead: {body}");
}

#[tokio::test]
async fn add_plugin_from_path_rejects_a_source_without_a_manifest() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    fs_err::create_dir_all(root.join("source")).expect("source dir");
    fs_err::create_dir_all(root.join("plugins")).expect("plugins root");
    let app = mock_app(plugin_state(&test_db, &root, root.join("no-host")).await);

    let (tag, body) = error_of(
        plugins::add_plugin_from_path(app.state::<AppState>(), root.join("source").to_string())
            .await,
    );
    assert_eq!(tag, "Invalid", "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("not a plugin directory"),
        "error did not explain the missing manifest: {body}"
    );
}

#[tokio::test]
async fn add_plugin_from_path_installs_and_returns_the_host_summary() {
    let host = host_binary();
    if !host.is_file() {
        eprintln!("skipping host-backed assertions: {} is not built", host);
        return;
    }
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let temp = tempfile::tempdir().expect("temp dir");
    let root = utf8(&temp);
    let source = root.join("source");
    write_plugin(&source, "sideloaded");
    fs_err::create_dir_all(root.join("plugins")).expect("plugins root");
    let app = mock_app(plugin_state(&test_db, &root, host).await);

    let summary = plugins::add_plugin_from_path(app.state::<AppState>(), source.to_string())
        .await
        .expect("the host loads the copied plugin");

    assert_eq!(summary.name, "sideloaded");
    assert_eq!(summary.version.as_deref(), Some("1.0.0"));
    assert!(summary.enabled, "a freshly installed plugin is enabled");
    assert!(
        summary.granted.iter().any(|grant| grant == "log"),
        "the host's grants were not reported: {summary:?}"
    );
    assert!(
        root.join("plugins")
            .join("sideloaded/plugin.toml")
            .is_file(),
        "the plugin was not copied into the scanned root"
    );
}

#[tokio::test]
async fn remote_registry_commands_report_the_feature_as_unavailable() {
    assert_unavailable(
        "discover_remote_plugins",
        plugins::discover_remote_plugins("https://plugins.example/index".to_string()).await,
    );
    assert_unavailable(
        "install_remote_plugin",
        plugins::install_remote_plugin(
            "https://plugins.example/index".to_string(),
            "txt-importer".to_string(),
        )
        .await,
    );

    let (_, body) = error_of(plugins::discover_remote_plugins(String::new()).await);
    assert!(
        body["message"]
            .as_str()
            .unwrap_or_default()
            .contains("no plugin registry"),
        "discovery did not explain the gate: {body}"
    );
}

// ---------------------------------------------------------------------------
// sync.rs
// ---------------------------------------------------------------------------

/// The fixture state's sync handle is an in-memory duplex whose daemon end is
/// dropped, i.e. a daemon that has gone away. Every wrapper must report that as
/// `SyncError::Unavailable` — not as a protocol error, and not as success.
#[tokio::test]
async fn sync_wrappers_map_a_dead_daemon_pipe_to_unavailable() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app(mock_state_for_bulk(&test_db).await);
    let state = || app.state::<AppState>();

    assert_unavailable("sync_health", sync::sync_health(state()).await);
    assert_unavailable("sync_status", sync::sync_status(state()).await);
    assert_unavailable(
        "sync_requests_recent",
        sync::sync_requests_recent(Some(5), state()).await,
    );
    assert_unavailable(
        "sync_pairing_begin",
        sync::sync_pairing_begin(Some("phone".to_string()), Some(30), state()).await,
    );
    assert_unavailable("sync_pairing_list", sync::sync_pairing_list(state()).await);
    assert_unavailable(
        "sync_pairing_approve",
        sync::sync_pairing_approve("tok-approve".to_string(), state()).await,
    );
    assert_unavailable(
        "sync_pairing_reject",
        sync::sync_pairing_reject("tok-reject".to_string(), state()).await,
    );
    assert_unavailable("sync_devices_list", sync::sync_devices_list(state()).await);
    assert_unavailable(
        "sync_devices_revoke",
        sync::sync_devices_revoke("dev-9".to_string(), state()).await,
    );
    assert_unavailable(
        "sync_conflicts_list",
        sync::sync_conflicts_list(state()).await,
    );
    assert_unavailable(
        "sync_conflicts_resolve",
        sync::sync_conflicts_resolve(
            7,
            "merge".to_string(),
            Some(r#"{"title":"x"}"#.to_string()),
            state(),
        )
        .await,
    );
    assert_unavailable("sync_server_start", sync::sync_server_start(state()).await);
    assert_unavailable("sync_server_stop", sync::sync_server_stop(state()).await);
}

// ---------------------------------------------------------------------------
// catalog.rs
// ---------------------------------------------------------------------------

struct Seed {
    work_id: DbId,
    edition_id: DbId,
}

/// One edition with a format, language, author, publisher, identifier and a
/// digital inventory row, i.e. every join `get_edition_detail` performs.
async fn seed_edition(db: &livtet_core::data::orm::DatabaseConnection) -> Seed {
    let now = now_primitive();
    let format_id = DbId::new();
    let language_id = DbId::new();

    formats::ActiveModel {
        id: Set(format_id),
        name: Set("EPUB".to_string()),
        metadata_schema: Set(serde_json::Value::Null),
        progress_unit: Set(None),
    }
    .insert(db)
    .await
    .expect("format");

    languages::ActiveModel {
        id: Set(language_id),
        name: Set("English".to_string()),
        code: Set("eng".to_string()),
        flag_emoji: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("language");

    let work_id = DbId::new();
    works::ActiveModel {
        id: Set(work_id),
        title: Set("The Book".to_string()),
        description: Set(Some("A work".to_string())),
        sort_title: Set(None),
        series_type: Set(None),
        language_id: Set(Some(language_id)),
        preferred_edition_id: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("work");

    let edition_id = DbId::new();
    editions::ActiveModel {
        id: Set(edition_id),
        work_id: Set(work_id),
        group_id: Set(None),
        title: Set(Some("The Book".to_string())),
        published_date: Set(Some(
            time::Date::from_calendar_date(2001, time::Month::September, 11).expect("date"),
        )),
        format_id: Set(Some(format_id)),
        language_id: Set(Some(language_id)),
        notes: Set(Some("A note".to_string())),
        description: Set(Some("An edition".to_string())),
        format_metadata: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("edition");

    let author_id = DbId::new();
    authors::ActiveModel {
        id: Set(author_id),
        name: Set("Ada Lovelace".to_string()),
    }
    .insert(db)
    .await
    .expect("author");
    edition_authors::ActiveModel {
        edition_id: Set(edition_id),
        author_id: Set(author_id),
        role: Set("aut".to_string()),
    }
    .insert(db)
    .await
    .expect("edition author");

    let publisher_id = DbId::new();
    publishers::ActiveModel {
        id: Set(publisher_id),
        name: Set("Acme Press".to_string()),
        website: Set(None),
        logo_url: Set(None),
        created_at: Set(now),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("publisher");
    edition_publishers::ActiveModel {
        edition_id: Set(edition_id),
        publisher_id: Set(publisher_id),
    }
    .insert(db)
    .await
    .expect("edition publisher");

    let identifier_id = DbId::new();
    identifiers::ActiveModel {
        id: Set(identifier_id),
        value: Set("urn:isbn:9780306406157".to_string()),
        kind: Set("isbn".to_string()),
    }
    .insert(db)
    .await
    .expect("identifier");
    edition_identifiers::ActiveModel {
        edition_id: Set(edition_id),
        identifier_id: Set(identifier_id),
    }
    .insert(db)
    .await
    .expect("edition identifier");

    digital_inventory::ActiveModel {
        id: Set(DbId::new()),
        edition_id: Set(edition_id),
        file_path: Set(Some("/books/the-book.epub".to_string())),
        cover_path: Set(Some("/covers/x/cover.jpg".to_string())),
        blurhash: Set(None),
        dominant_color: Set(None),
        file_hash: Set(Some("deadbeef".to_string())),
        file_size_bytes: Set(Some(1024)),
        file_format: Set(Some("epub".to_string())),
        notes: Set(None),
        added_at: Set(now),
        updated_at: Set(None),
    }
    .insert(db)
    .await
    .expect("digital inventory");

    Seed {
        work_id,
        edition_id,
    }
}

#[tokio::test]
async fn get_edition_detail_wrapper_parses_the_id_and_assembles_the_record() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let seed = seed_edition(&db).await;
    let app = mock_app(mock_state_for_bulk(&test_db).await);

    let detail = catalog::get_edition_detail(seed.edition_id.to_string(), app.state::<AppState>())
        .await
        .expect("query ok")
        .expect("the seeded edition exists");

    assert_eq!(detail.id, seed.edition_id.to_string());
    assert_eq!(detail.work_id, seed.work_id.to_string());
    assert_eq!(detail.title.as_deref(), Some("The Book"));
    assert_eq!(detail.published_date.as_deref(), Some("2001-09-11"));
    assert_eq!(detail.format.as_deref(), Some("EPUB"));
    assert_eq!(detail.language_name.as_deref(), Some("English"));
    assert_eq!(detail.authors.len(), 1);
    assert_eq!(detail.authors[0].name, "Ada Lovelace");
    assert_eq!(detail.authors[0].role_label, "Author");
    assert_eq!(detail.publishers, vec!["Acme Press".to_string()]);
    assert_eq!(
        detail.identifiers,
        vec![catalog::IdentifierRef {
            kind: "isbn".to_string(),
            value: "urn:isbn:9780306406157".to_string(),
        }]
    );
    let file = detail.file.expect("inventory row became a file entry");
    assert_eq!(file.file_format.as_deref(), Some("epub"));
    assert_eq!(file.file_size_bytes, Some(1024.0));
    assert_eq!(file.cover_path.as_deref(), Some("/covers/x/cover.jpg"));
    assert_eq!(file.file_status, Some(catalog::FileStatus::Missing));
}

#[tokio::test]
async fn get_edition_detail_wrapper_rejects_an_unparseable_id_before_querying() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let app = mock_app(mock_state_for_bulk(&test_db).await);
    let state = || app.state::<AppState>();

    let (tag, body) = error_of(catalog::get_edition_detail("not-an-id".to_string(), state()).await);
    assert_eq!(tag, "InvalidId", "{body}");
    assert_eq!(body["id"], Value::String("not-an-id".to_string()));

    // A well-formed id that matches nothing is `None`, not an error.
    let missing = catalog::get_edition_detail(DbId::new().to_string(), state())
        .await
        .expect("a missing edition is not a failure");
    assert!(missing.is_none());
}

#[tokio::test]
async fn get_edition_covers_wrapper_resolves_paths_and_rejects_bad_ids() {
    let test_db = TestDb::new(&[Kind::Business]).await.expect("db");
    let db = test_db.state().db_conn();
    let seed = seed_edition(&db).await;
    let app = mock_app(mock_state_for_bulk(&test_db).await);
    let state = || app.state::<AppState>();

    let covers = catalog::get_edition_covers(vec![seed.edition_id.to_string()], state())
        .await
        .expect("query ok");
    assert_eq!(
        covers,
        vec![catalog::EditionCover {
            edition_id: seed.edition_id.to_string(),
            cover_path: "/covers/x/cover.jpg".to_string(),
        }]
    );

    assert!(
        catalog::get_edition_covers(Vec::new(), state())
            .await
            .expect("an empty batch is not an error")
            .is_empty()
    );

    // Fail closed: one unparseable id fails the whole batch rather than
    // silently dropping that entry.
    let (tag, body) = error_of(
        catalog::get_edition_covers(
            vec![seed.edition_id.to_string(), "bogus".to_string()],
            state(),
        )
        .await,
    );
    assert_eq!(tag, "InvalidId", "{body}");
    assert_eq!(body["id"], Value::String("bogus".to_string()));
}
