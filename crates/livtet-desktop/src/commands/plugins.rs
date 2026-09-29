use std::path::{Path, PathBuf};

use serde::Serialize;
use specta::Type;
use tauri::State;

use crate::error::PluginError;
use crate::types::AppState;

/// A plugin as reported by the out-of-process host.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PluginSummary {
    pub name: String,
    pub version: Option<String>,
    pub granted: Vec<String>,
    pub signer: String,
}

impl From<stanchion::remote::PluginInfo> for PluginSummary {
    fn from(plugin: stanchion::remote::PluginInfo) -> Self {
        Self {
            name: plugin.name,
            version: plugin.version,
            granted: plugin.granted,
            signer: plugin.signer,
        }
    }
}

/// A plugin offered by a remote registry, before installation.
#[derive(Debug, Clone, Serialize, Type)]
pub struct RemotePlugin {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub signer: Option<String>,
}

/// Launches the host against the plugins directory and returns the loaded set.
///
/// The host binary owns the registry, so it is launched, queried and dropped
/// per call: a plugin that crashes the interpreter takes the host with it, and
/// the next call gets a fresh process. `RemoteRegistry` is not `Send`, so this
/// runs on a blocking worker and only owned data crosses back into async land.
fn list_via_host(
    host: PathBuf,
    config: PathBuf,
    plugins_dir: PathBuf,
) -> Result<Vec<PluginSummary>, PluginError> {
    let options = stanchion::remote::RemoteOptions::new(host)
        .config(config)
        .plugins(&plugins_dir);
    let mut registry =
        stanchion::remote::RemoteRegistry::launch(options).map_err(PluginError::host)?;
    let plugins = registry.list().map_err(PluginError::host)?;
    Ok(plugins.into_iter().map(PluginSummary::from).collect())
}

/// Rejects names that are not a single, safe path segment, so a plugin name from
/// a manifest or the frontend can never escape the plugins directory.
fn ensure_safe_name(name: &str) -> Result<(), PluginError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(PluginError::invalid(format!(
            "unsafe plugin name: {name:?}"
        )));
    }
    Ok(())
}

/// Copies a plugin source tree, rejecting symbolic links.
///
/// `entry.file_type()` does not follow links, so a symlink lands in the `else`
/// branch where `std::fs::copy` *would* follow it and pull external bytes (e.g. a
/// link to `/etc/passwd`) into the plugins directory. Refusing symlinks keeps an
/// installed plugin's directory a faithful, self-contained copy of its source.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name());
        if file_type.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "symbolic links are not allowed in plugins: {}",
                    entry.path().display()
                ),
            ));
        } else if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The one manifest field install needs: the plugin's name determines its
/// directory. The host validates the rest of the manifest when it loads the
/// plugin, so this deliberately parses only `name`.
#[derive(serde::Deserialize)]
struct ManifestName {
    name: String,
}

/// Reads the declared plugin name from `<source>/plugin.toml`.
fn read_plugin_name(source: &Path) -> Result<String, PluginError> {
    let manifest_path = source.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest_path).map_err(|err| {
        PluginError::invalid(format!(
            "not a plugin directory ({}): {err}",
            manifest_path.display()
        ))
    })?;
    let manifest: ManifestName = toml::from_str(&text)
        .map_err(|err| PluginError::invalid(format!("{}: {err}", manifest_path.display())))?;
    Ok(manifest.name)
}

/// Validates a plugin source directory and copies it into the plugins root under
/// the name its manifest declares. Returns that name.
///
/// The name is read from the manifest and checked to be a safe path segment, and
/// an existing install with the same name is refused rather than silently
/// overwritten. Full manifest validation happens when the host loads the plugin.
fn install_plugin_dir(plugins_dir: &Path, source: &Path) -> Result<String, PluginError> {
    let name = read_plugin_name(source)?;
    ensure_safe_name(&name)?;

    let dest = plugins_dir.join(&name);
    if dest.exists() {
        return Err(PluginError::invalid(format!(
            "a plugin named \"{name}\" is already installed"
        )));
    }
    std::fs::create_dir_all(plugins_dir).map_err(PluginError::host)?;
    if let Err(err) = copy_dir_recursive(source, &dest) {
        // A partial copy must not linger as a half-installed plugin.
        let _ = std::fs::remove_dir_all(&dest);
        return Err(PluginError::invalid(format!(
            "could not copy plugin from {}: {err}",
            source.display()
        )));
    }
    Ok(name)
}

/// Removes an installed plugin's directory. Uninstalling is how a plugin's access
/// is durably revoked: the host is launched per call, so it carries no state
/// between calls to revoke against.
fn remove_plugin_dir(plugins_dir: &Path, name: &str) -> Result<(), PluginError> {
    ensure_safe_name(name)?;
    let dest = plugins_dir.join(name);
    if !dest.is_dir() {
        return Err(PluginError::invalid(format!(
            "no plugin named \"{name}\" is installed"
        )));
    }
    std::fs::remove_dir_all(&dest).map_err(PluginError::host)?;
    Ok(())
}

/// Lists the plugins the host loads from the application's plugin directory.
#[tauri::command]
#[specta::specta]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginSummary>, PluginError> {
    let host = state.plugin_host_path.clone().into_std_path_buf();
    let config = state.plugin_host_config.clone().into_std_path_buf();
    let plugins_dir = state.plugins_dir.clone().into_std_path_buf();

    tokio::task::spawn_blocking(move || list_via_host(host, config, plugins_dir))
        .await
        .map_err(PluginError::host)?
}

/// Copies a plugin into the plugins root and confirms the host loads it,
/// returning its summary. Any failure after the copy — the host erroring, or the
/// plugin loading but not appearing — rolls the copy back, so the plugins
/// directory is never left with a broken plugin.
fn install_and_verify(
    host: PathBuf,
    config: PathBuf,
    plugins_dir: PathBuf,
    source: &Path,
) -> Result<PluginSummary, PluginError> {
    let name = install_plugin_dir(&plugins_dir, source)?;
    let rollback = || {
        let _ = std::fs::remove_dir_all(plugins_dir.join(&name));
    };

    let loaded = match list_via_host(host, config, plugins_dir.clone()) {
        Ok(loaded) => loaded,
        Err(err) => {
            rollback();
            return Err(err);
        }
    };
    match loaded.into_iter().find(|plugin| plugin.name == name) {
        Some(summary) => Ok(summary),
        None => {
            rollback();
            Err(PluginError::invalid(format!(
                "plugin \"{name}\" was installed but the host could not load it"
            )))
        }
    }
}

/// Installs a plugin from a local directory (developer / manual sideload), then
/// confirms the host can load it and returns its summary.
#[tauri::command]
#[specta::specta]
pub async fn add_plugin_from_path(
    state: State<'_, AppState>,
    source: String,
) -> Result<PluginSummary, PluginError> {
    let host = state.plugin_host_path.clone().into_std_path_buf();
    let config = state.plugin_host_config.clone().into_std_path_buf();
    let plugins_dir = state.plugins_dir.clone().into_std_path_buf();
    let source = PathBuf::from(source);

    tokio::task::spawn_blocking(move || install_and_verify(host, config, plugins_dir, &source))
        .await
        .map_err(PluginError::host)?
}

/// Uninstalls a plugin by name, removing its directory.
#[tauri::command]
#[specta::specta]
pub async fn remove_plugin(state: State<'_, AppState>, name: String) -> Result<(), PluginError> {
    let plugins_dir = state.plugins_dir.clone().into_std_path_buf();
    tokio::task::spawn_blocking(move || remove_plugin_dir(&plugins_dir, &name))
        .await
        .map_err(PluginError::host)?
}

/// Browses a remote plugin registry for installable plugins.
///
/// Remote discovery is gated on a reachable registry endpoint
/// (`stanchion-index-poem`); none ships yet, so this reports the feature as
/// unavailable. The command and its typed shape exist so the UI can offer the
/// "from repository" flow and degrade cleanly. See ADR-0030.
#[tauri::command]
#[specta::specta]
pub async fn discover_remote_plugins(
    registry_url: String,
) -> Result<Vec<RemotePlugin>, PluginError> {
    let _ = registry_url;
    Err(PluginError::unavailable(
        "no plugin registry is configured yet",
    ))
}

/// Installs a plugin from a remote registry by id. Gated like
/// [`discover_remote_plugins`].
#[tauri::command]
#[specta::specta]
pub async fn install_remote_plugin(
    registry_url: String,
    id: String,
) -> Result<PluginSummary, PluginError> {
    let _ = (registry_url, id);
    Err(PluginError::unavailable(
        "no plugin registry is configured yet",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use std::path::PathBuf;

    use super::{
        PluginError, install_and_verify, install_plugin_dir, list_via_host, remove_plugin_dir,
    };

    /// A Lua importer the host can actually load, for host-backed tests.
    const LOADABLE_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer
function Importer.new(config, deps) return setmetatable({}, Importer) end
function Importer:extensions() return { "txt" } end
function Importer:read_metadata(path) return { title = "T" } end
return Importer
"#;

    fn host_binary() -> PathBuf {
        let test_binary = std::env::current_exe().unwrap();
        let profile_dir = test_binary.parent().and_then(|path| path.parent()).unwrap();
        profile_dir.join(if cfg!(windows) {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        })
    }

    fn write_host_config(dir: &Path) -> PathBuf {
        let config = dir.join("host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"fs_read\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();
        config
    }

    fn write_loadable_plugin(plugins_dir: &Path, name: &str) -> PathBuf {
        let plugin_dir = plugins_dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            format!("name = \"{name}\"\nversion = \"1.0.0\"\n\n[capabilities.log]\n"),
        )
        .unwrap();
        fs::write(plugin_dir.join("init.lua"), LOADABLE_IMPORTER).unwrap();
        plugin_dir
    }

    fn write_plugin(plugins_dir: &Path, name: &str) -> std::path::PathBuf {
        let plugin_dir = plugins_dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\n\n[capabilities.log]\n[capabilities.fs_read]\n"
            ),
        )
        .unwrap();
        fs::write(plugin_dir.join("init.lua"), "return {}\n").unwrap();
        plugin_dir
    }

    #[test]
    fn install_names_destination_by_manifest_and_copies_files() {
        let root = tempfile::tempdir().unwrap();
        let source = write_plugin(&root.path().join("src"), "txt-importer");
        let plugins_dir = root.path().join("plugins");

        let name = install_plugin_dir(&plugins_dir, &source).expect("install succeeds");

        assert_eq!(name, "txt-importer");
        assert!(plugins_dir.join("txt-importer/plugin.toml").is_file());
        assert!(plugins_dir.join("txt-importer/init.lua").is_file());
    }

    #[test]
    fn install_refuses_a_duplicate_name() {
        let root = tempfile::tempdir().unwrap();
        let source = write_plugin(&root.path().join("src"), "dup");
        let plugins_dir = root.path().join("plugins");

        install_plugin_dir(&plugins_dir, &source).expect("first install");
        let err = install_plugin_dir(&plugins_dir, &source).expect_err("second install fails");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn install_rejects_a_source_without_a_manifest() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("empty");
        fs::create_dir_all(&source).unwrap();
        let plugins_dir = root.path().join("plugins");

        let err = install_plugin_dir(&plugins_dir, &source).expect_err("no manifest fails");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn remove_deletes_an_installed_plugin() {
        let root = tempfile::tempdir().unwrap();
        let source = write_plugin(&root.path().join("src"), "gone");
        let plugins_dir = root.path().join("plugins");
        install_plugin_dir(&plugins_dir, &source).unwrap();

        remove_plugin_dir(&plugins_dir, "gone").expect("remove succeeds");
        assert!(!plugins_dir.join("gone").exists());
    }

    #[test]
    fn remove_rejects_path_traversal_and_deletes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = root.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        let sentinel = root.path().join("keep.txt");
        fs::write(&sentinel, b"keep").unwrap();

        for evil in ["..", "../keep.txt", "../..", "nested/child"] {
            let err = remove_plugin_dir(&plugins_dir, evil).expect_err("traversal rejected");
            assert!(
                matches!(err, PluginError::Invalid { .. }),
                "{evil}: {err:?}"
            );
        }
        assert!(sentinel.is_file(), "sentinel outside plugins dir survived");
    }

    // Regression: list_via_host must not pass args the host rejects. The host
    // only accepts --config/--plugins, so a stray `--contract report` made it
    // exit before serving and every list/add call failed.
    #[test]
    fn list_via_host_lists_an_installed_plugin() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = root.path().join("plugins");
        write_loadable_plugin(&plugins_dir, "txt-importer");
        let config = write_host_config(root.path());

        let listed = list_via_host(host_binary(), config, plugins_dir)
            .expect("host lists the installed plugin");
        assert!(
            listed.iter().any(|plugin| plugin.name == "txt-importer"),
            "expected txt-importer in {listed:?}"
        );
    }

    // Regression: a post-copy failure (here, an unreachable host) must roll the
    // copied directory back instead of leaving a half-installed plugin.
    #[test]
    fn install_and_verify_rolls_back_when_the_host_is_unreachable() {
        let root = tempfile::tempdir().unwrap();
        let source = write_loadable_plugin(&root.path().join("src"), "rollback-me");
        let plugins_dir = root.path().join("plugins");
        let bogus_host = root.path().join("does-not-exist-host");
        let config = write_host_config(root.path());

        let err = install_and_verify(bogus_host, config, plugins_dir.clone(), &source)
            .expect_err("unreachable host fails the install");
        assert!(matches!(err, PluginError::Host { .. }), "got {err:?}");
        assert!(
            !plugins_dir.join("rollback-me").exists(),
            "copied plugin directory was not rolled back"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_rejects_a_symlink_in_the_source() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("secret.txt");
        fs::write(&outside, b"secret").unwrap();
        let source = write_plugin(&root.path().join("src"), "sneaky");
        std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
        let plugins_dir = root.path().join("plugins");

        let err = install_plugin_dir(&plugins_dir, &source).expect_err("symlink rejected");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
        assert!(
            !plugins_dir.join("sneaky").exists(),
            "partial copy was not cleaned up"
        );
    }

    // The `xml` sandbox module: livtet-plugin-host registers it in-host, so a
    // plugin that declares `[capabilities.xml]` can parse XML via `xml.parse`
    // without vendoring a native module.
    #[test]
    fn xml_capability_lets_a_plugin_parse() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = root.path().join("plugins");
        let plugin_dir = plugins_dir.join("xml-probe");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"xml-probe\"\nversion = \"1.0.0\"\n\n[capabilities.xml]\n",
        )
        .unwrap();
        fs::write(
            plugin_dir.join("init.lua"),
            r#"
local Probe = {}
Probe.__index = Probe
function Probe.new(config, deps) return setmetatable({}, Probe) end
function Probe:root(source)
  local doc = xml.parse(source)
  local first = doc.children[1]
  return doc.tag .. "/" .. (first and first.tag or "") .. "=" .. (first and first.attrs.id or "")
end
return Probe
"#,
        )
        .unwrap();

        let config = root.path().join("host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"xml\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary())
            .config(config)
            .plugins(&plugins_dir);
        let mut remote =
            stanchion::remote::RemoteRegistry::launch(options).expect("host launches");
        let out: serde_json::Value = remote
            .call(
                "xml-probe",
                "root",
                [serde_json::json!("<package><metadata id=\"m1\"/></package>")],
            )
            .expect("plugin parses xml");
        assert_eq!(out, serde_json::json!("package/metadata=m1"));
    }

    // The `sqlite` sandbox module: livtet-plugin-host opens read-only, and only
    // the database paths granted via `--sqlite`. A plugin declaring
    // `[capabilities.sqlite]` queries a granted DB; a non-granted path is refused.
    #[test]
    fn sqlite_capability_queries_only_granted_databases() {
        let root = tempfile::tempdir().unwrap();
        let db_path = root.path().join("books.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE books(id INTEGER, title TEXT);\
                 INSERT INTO books VALUES (1, 'Dune'), (2, 'Neuromancer');",
            )
            .unwrap();
        }
        let other_db = root.path().join("secret.db");
        rusqlite::Connection::open(&other_db).unwrap();

        let plugins_dir = root.path().join("plugins");
        let plugin_dir = plugins_dir.join("sqlite-probe");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"sqlite-probe\"\nversion = \"1.0.0\"\n\n[capabilities.sqlite]\n",
        )
        .unwrap();
        fs::write(
            plugin_dir.join("init.lua"),
            r#"
local P = {}
P.__index = P
function P.new(config, deps) return setmetatable({}, P) end
function P:titles(db)
  local rows = sqlite.query(db, "SELECT title FROM books ORDER BY id", {})
  local out = {}
  for i, row in ipairs(rows) do out[i] = row.title end
  return table.concat(out, ",")
end
function P:forbidden(db)
  local ok, err = pcall(function() return sqlite.query(db, "SELECT 1", {}) end)
  return ok
end
return P
"#,
        )
        .unwrap();

        let config = root.path().join("host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"sqlite\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary())
            .config(config)
            .plugins(&plugins_dir)
            .arg("--sqlite")
            .arg(db_path.as_os_str());
        let mut remote =
            stanchion::remote::RemoteRegistry::launch(options).expect("host launches");

        let titles: serde_json::Value = remote
            .call(
                "sqlite-probe",
                "titles",
                [serde_json::json!(db_path.to_str().unwrap())],
            )
            .expect("query the granted database");
        assert_eq!(titles, serde_json::json!("Dune,Neuromancer"));

        let allowed: serde_json::Value = remote
            .call(
                "sqlite-probe",
                "forbidden",
                [serde_json::json!(other_db.to_str().unwrap())],
            )
            .expect("call completes");
        assert_eq!(allowed, serde_json::json!(false), "non-granted db refused");
    }

    // The `http` sandbox module: a plugin's `http(request)` is forwarded to the
    // app, which performs the fetch under its allowlist and returns the reply.
    #[test]
    fn http_capability_forwards_to_the_app() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::{Read, Write};
                let _ = stream.read(&mut [0u8; 1024]);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nContent-Type: text/plain\r\n\r\npong",
                );
            }
        });

        let root = tempfile::tempdir().unwrap();
        let plugins_dir = root.path().join("plugins");
        let plugin_dir = plugins_dir.join("http-probe");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"http-probe\"\nversion = \"1.0.0\"\n\n[capabilities.http]\n",
        )
        .unwrap();
        fs::write(
            plugin_dir.join("init.lua"),
            r#"
local P = {}
P.__index = P
function P.new(config, deps) return setmetatable({}, P) end
function P:fetch(url)
  local resp = http({ url = url })
  return resp.status .. ":" .. resp.body
end
return P
"#,
        )
        .unwrap();

        let config = root.path().join("host.toml");
        fs::write(
            &config,
            "[capabilities]\ncallbacks = [\"http\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary())
            .config(config)
            .plugins(&plugins_dir);
        let remote = stanchion::remote::RemoteRegistry::launch(options).expect("host launches");
        let allow = vec!["127.0.0.1".to_string()];
        let mut remote = remote.on_callback(move |call| {
            crate::commands::plugin_host_callbacks::answer_http(call, &allow)
        });

        let out: serde_json::Value = remote
            .call(
                "http-probe",
                "fetch",
                [serde_json::json!(format!("http://{addr}/"))],
            )
            .expect("plugin fetches via http");
        assert_eq!(out, serde_json::json!("200:pong"));
        server.join().unwrap();
    }
}
