use camino::{Utf8Path, Utf8PathBuf};

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
    /// Whether the plugin's directory lives in the root the host scans for work.
    /// See ADR-0034.
    pub enabled: bool,
}

impl From<stanchion::remote::PluginInfo> for PluginSummary {
    /// The host reports no enabled state — it reports what it parsed from the
    /// root it was pointed at. Which root that was is the caller's knowledge, so
    /// a conversion defaults to enabled and [`list_all_via_host`] flips the
    /// plugins it read from the disabled root.
    fn from(plugin: stanchion::remote::PluginInfo) -> Self {
        Self {
            name: plugin.name,
            version: plugin.version,
            granted: plugin.granted,
            signer: plugin.signer,
            enabled: true,
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
    host: Utf8PathBuf,
    config: Utf8PathBuf,
    plugins_dir: Utf8PathBuf,
) -> Result<Vec<PluginSummary>, PluginError> {
    // `RemoteOptions` takes std paths at this boundary.
    let options = stanchion::remote::RemoteOptions::new(host.into_std_path_buf())
        .config(config)
        .plugins(&plugins_dir);
    let mut registry =
        stanchion::remote::RemoteRegistry::launch(options).map_err(PluginError::host)?;
    let plugins = registry.list().map_err(PluginError::host)?;
    Ok(plugins.into_iter().map(PluginSummary::from).collect())
}

/// Whether a root holds at least one plugin directory.
///
/// A second host launch is only worth paying for when there is something in the
/// disabled root to parse; a missing root (nothing has ever been disabled) reads
/// as empty rather than as an error.
fn has_plugin_dirs(root: &Utf8Path) -> bool {
    std::fs::read_dir(root).is_ok_and(|mut entries| {
        entries.any(|entry| entry.is_ok_and(|entry| entry.path().is_dir()))
    })
}

/// Lists both plugin roots, marking everything from the disabled root
/// `enabled: false`.
///
/// The host is launched once per root (ADR-0034). A disabled plugin therefore
/// keeps reporting its real name, version, signer and grants, because the host
/// still parses it — it is simply never loaded for work, since the root it sits
/// in is not the one the app points the host at elsewhere.
fn list_all_via_host(
    host: Utf8PathBuf,
    config: Utf8PathBuf,
    plugins_dir: Utf8PathBuf,
    disabled_dir: Utf8PathBuf,
) -> Result<Vec<PluginSummary>, PluginError> {
    let mut plugins = list_via_host(host.clone(), config.clone(), plugins_dir)?;
    if has_plugin_dirs(&disabled_dir) {
        plugins.extend(
            list_via_host(host, config, disabled_dir)?
                .into_iter()
                .map(|plugin| PluginSummary {
                    enabled: false,
                    ..plugin
                }),
        );
    }
    Ok(plugins)
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
fn copy_dir_recursive(src: &Utf8Path, dst: &Utf8Path) -> std::io::Result<()> {
    fs_err::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = dst.join(entry.file_name().to_string_lossy().as_ref());
        if file_type.is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "symbolic links are not allowed in plugins: {}",
                    entry.path().display()
                ),
            ));
        } else if file_type.is_dir() {
            let sub = Utf8PathBuf::from_path_buf(entry.path()).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "non-UTF-8 path")
            })?;
            copy_dir_recursive(&sub, &target)?;
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
fn read_plugin_name(source: &Utf8Path) -> Result<String, PluginError> {
    let manifest_path = source.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest_path).map_err(|err| {
        PluginError::invalid(format!("not a plugin directory ({manifest_path}): {err}"))
    })?;
    let manifest: ManifestName = toml::from_str(&text)
        .map_err(|err| PluginError::invalid(format!("{manifest_path}: {err}")))?;
    Ok(manifest.name)
}

/// Validates a plugin source directory and copies it into the plugins root under
/// the name its manifest declares. Returns that name.
///
/// The name is read from the manifest and checked to be a safe path segment, and
/// an existing install with the same name is refused rather than silently
/// overwritten. Full manifest validation happens when the host loads the plugin.
fn install_plugin_dir(plugins_dir: &Utf8Path, source: &Utf8Path) -> Result<String, PluginError> {
    let name = read_plugin_name(source)?;
    ensure_safe_name(&name)?;

    let dest = plugins_dir.join(name.as_str());
    if dest.exists() {
        return Err(PluginError::invalid(format!(
            "a plugin named \"{name}\" is already installed"
        )));
    }
    fs_err::create_dir_all(plugins_dir).map_err(PluginError::host)?;
    if let Err(err) = copy_dir_recursive(source, &dest) {
        // A partial copy must not linger as a half-installed plugin.
        let _ = std::fs::remove_dir_all(&dest);
        return Err(PluginError::invalid(format!(
            "could not copy plugin from {source}: {err}"
        )));
    }
    Ok(name)
}

/// Removes an installed plugin's directory. Uninstalling is how a plugin's access
/// is durably revoked: the host is launched per call, so it carries no state
/// between calls to revoke against.
fn remove_plugin_dir(plugins_dir: &Utf8Path, name: &str) -> Result<(), PluginError> {
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

/// Removes a plugin from whichever root holds it.
///
/// Two roots hold plugin directories now, so anything reasoning about "installed
/// plugins" must consider both (ADR-0034) — otherwise uninstalling a disabled
/// plugin silently does nothing. The error for a name in neither root is the same
/// one [`remove_plugin_dir`] has always given.
fn remove_plugin_from_roots(
    plugins_dir: &Utf8Path,
    disabled_dir: &Utf8Path,
    name: &str,
) -> Result<(), PluginError> {
    ensure_safe_name(name)?;
    if plugins_dir.join(name).is_dir() {
        remove_plugin_dir(plugins_dir, name)
    } else {
        remove_plugin_dir(disabled_dir, name)
    }
}

/// Moves a plugin's directory between the scanned root and the disabled root.
///
/// Disabling moves `<name>` out of `plugins_dir`; enabling moves it back. The
/// loaded set is exactly the directories under `plugins_dir`, so a moved-out
/// plugin is unreachable by the same mechanism as an uninstalled one, while its
/// files and grant decisions survive and the state survives a restart with no new
/// store (ADR-0034). The two roots are siblings in the app directory, so the
/// rename never crosses a filesystem.
///
/// Idempotent: a plugin already in the requested root is left alone, so toggling
/// a switch twice is not an error.
fn move_plugin_between_roots(
    plugins_dir: &Utf8Path,
    disabled_dir: &Utf8Path,
    name: &str,
    enabled: bool,
) -> Result<(), PluginError> {
    ensure_safe_name(name)?;
    let (from, to) = if enabled {
        (disabled_dir.join(name), plugins_dir.join(name))
    } else {
        (plugins_dir.join(name), disabled_dir.join(name))
    };
    if to.is_dir() {
        return Ok(());
    }
    if !from.is_dir() {
        return Err(PluginError::invalid(format!(
            "no plugin named \"{name}\" is installed"
        )));
    }
    if let Some(parent) = to.parent() {
        fs_err::create_dir_all(parent).map_err(PluginError::host)?;
    }
    std::fs::rename(&from, &to).map_err(PluginError::host)?;
    Ok(())
}

/// Lists the plugins in both plugin roots: those the host loads for work, and
/// those parked in the disabled root, reported with `enabled: false`.
#[tauri::command]
#[specta::specta]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginSummary>, PluginError> {
    let host = state.plugin_host_path.clone();
    let config = state.plugin_host_config.clone();
    let plugins_dir = state.plugins_dir.clone();
    let disabled_dir = state.disabled_plugins_dir.clone();

    tokio::task::spawn_blocking(move || list_all_via_host(host, config, plugins_dir, disabled_dir))
        .await
        .map_err(PluginError::host)?
}

/// Enables or disables a plugin by moving its directory between the scanned and
/// disabled plugin roots. Idempotent, and safe against names that would escape a
/// root. See ADR-0034.
///
/// Not synchronised against an in-flight `list_plugins`: the host is launched per
/// call and calls are short, so a plugin moved mid-call fails that one call.
#[tauri::command]
#[specta::specta]
pub async fn set_plugin_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), PluginError> {
    let plugins_dir = state.plugins_dir.clone();
    let disabled_dir = state.disabled_plugins_dir.clone();

    tokio::task::spawn_blocking(move || {
        move_plugin_between_roots(&plugins_dir, &disabled_dir, &name, enabled)
    })
    .await
    .map_err(PluginError::host)?
}

/// Copies a plugin into the plugins root and confirms the host loads it,
/// returning its summary. Any failure after the copy — the host erroring, or the
/// plugin loading but not appearing — rolls the copy back, so the plugins
/// directory is never left with a broken plugin.
fn install_and_verify(
    host: Utf8PathBuf,
    config: Utf8PathBuf,
    plugins_dir: Utf8PathBuf,
    source: &Utf8Path,
) -> Result<PluginSummary, PluginError> {
    let name = install_plugin_dir(&plugins_dir, source)?;
    let rollback = || {
        let _ = std::fs::remove_dir_all(plugins_dir.join(name.as_str()));
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
    let host = state.plugin_host_path.clone();
    let config = state.plugin_host_config.clone();
    let plugins_dir = state.plugins_dir.clone();
    let source = Utf8PathBuf::from(source);

    tokio::task::spawn_blocking(move || install_and_verify(host, config, plugins_dir, &source))
        .await
        .map_err(PluginError::host)?
}

/// Uninstalls a plugin by name, removing its directory from whichever root holds
/// it — a disabled plugin is still installed, and must still be uninstallable.
#[tauri::command]
#[specta::specta]
pub async fn remove_plugin(state: State<'_, AppState>, name: String) -> Result<(), PluginError> {
    let plugins_dir = state.plugins_dir.clone();
    let disabled_dir = state.disabled_plugins_dir.clone();
    tokio::task::spawn_blocking(move || {
        remove_plugin_from_roots(&plugins_dir, &disabled_dir, &name)
    })
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
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use camino::{Utf8Path, Utf8PathBuf};
    use fs_err as fs;

    use super::{
        PluginError, install_and_verify, install_plugin_dir, list_all_via_host, list_via_host,
        move_plugin_between_roots, remove_plugin_dir, remove_plugin_from_roots,
    };

    /// Join `name` onto a tempdir and borrow it as UTF-8.
    fn dir(root: &tempfile::TempDir, name: &str) -> Utf8PathBuf {
        Utf8PathBuf::from(root.path().to_str().unwrap()).join(name)
    }

    /// A Lua importer the host can actually load, for host-backed tests.
    const LOADABLE_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer
function Importer.new(config, deps) return setmetatable({}, Importer) end
function Importer:extensions() return { "txt" } end
function Importer:read_metadata(path) return { title = "T" } end
return Importer
"#;

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

    fn write_host_config(root: &tempfile::TempDir) -> Utf8PathBuf {
        let config = dir(root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"fs_read\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();
        config
    }

    fn write_loadable_plugin(plugins_dir: &Utf8Path, name: &str) -> Utf8PathBuf {
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

    fn write_plugin(plugins_dir: &Utf8Path, name: &str) -> Utf8PathBuf {
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
        let source = write_plugin(&dir(&root, "src"), "txt-importer");
        let plugins_dir = dir(&root, "plugins");

        let name = install_plugin_dir(&plugins_dir, &source).expect("install succeeds");

        assert_eq!(name, "txt-importer");
        assert!(plugins_dir.join("txt-importer/plugin.toml").is_file());
        assert!(plugins_dir.join("txt-importer/init.lua").is_file());
    }

    #[test]
    fn install_refuses_a_duplicate_name() {
        let root = tempfile::tempdir().unwrap();
        let source = write_plugin(&dir(&root, "src"), "dup");
        let plugins_dir = dir(&root, "plugins");

        install_plugin_dir(&plugins_dir, &source).expect("first install");
        let err = install_plugin_dir(&plugins_dir, &source).expect_err("second install fails");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn install_rejects_a_source_without_a_manifest() {
        let root = tempfile::tempdir().unwrap();
        let source = dir(&root, "empty");
        fs::create_dir_all(&source).unwrap();
        let plugins_dir = dir(&root, "plugins");

        let err = install_plugin_dir(&plugins_dir, &source).expect_err("no manifest fails");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    #[test]
    fn remove_deletes_an_installed_plugin() {
        let root = tempfile::tempdir().unwrap();
        let source = write_plugin(&dir(&root, "src"), "gone");
        let plugins_dir = dir(&root, "plugins");
        install_plugin_dir(&plugins_dir, &source).unwrap();

        remove_plugin_dir(&plugins_dir, "gone").expect("remove succeeds");
        assert!(!plugins_dir.join("gone").exists());
    }

    #[test]
    fn remove_rejects_path_traversal_and_deletes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        fs::create_dir_all(&plugins_dir).unwrap();
        let sentinel = dir(&root, "keep.txt");
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

    // ADR-0034: disabling moves the plugin's directory out of the scanned root
    // and enabling moves it back. Nothing is marked; the filesystem is the
    // registry.
    #[test]
    fn set_plugin_enabled_moves_between_roots() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        write_plugin(&plugins_dir, "movable");

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "movable", false)
            .expect("disable succeeds");
        assert!(
            !plugins_dir.join("movable").exists(),
            "disabled plugin stayed in the scanned root"
        );
        assert!(
            disabled_dir.join("movable/plugin.toml").is_file(),
            "the plugin's files did not move intact"
        );

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "movable", true)
            .expect("enable succeeds");
        assert!(plugins_dir.join("movable/plugin.toml").is_file());
        assert!(!disabled_dir.join("movable").exists());

        let err = move_plugin_between_roots(&plugins_dir, &disabled_dir, "never-installed", false)
            .expect_err("a plugin in neither root cannot be toggled");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    // The host loads exactly the directories under the scanned root, so a
    // disabled plugin is unreachable by the same mechanism as an uninstalled one.
    #[test]
    fn disabled_plugin_is_not_listed_as_enabled() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        write_loadable_plugin(&plugins_dir, "txt-importer");
        let config = write_host_config(&root);

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "txt-importer", false)
            .expect("disable succeeds");

        let listed = list_via_host(host_binary(), config, plugins_dir).expect("host still lists");
        assert!(
            !listed.iter().any(|plugin| plugin.name == "txt-importer"),
            "disabled plugin was still loaded: {listed:?}"
        );
    }

    // A disabled plugin keeps reporting real metadata, because the host still
    // parses it from the second root; it is simply never loaded for work.
    #[test]
    fn list_plugins_includes_disabled_with_metadata() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        write_loadable_plugin(&plugins_dir, "stays-on");
        write_loadable_plugin(&plugins_dir, "turned-off");
        let config = write_host_config(&root);

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "turned-off", false)
            .expect("disable succeeds");

        let listed = list_all_via_host(host_binary(), config, plugins_dir, disabled_dir)
            .expect("both roots are listed");

        let on = listed
            .iter()
            .find(|plugin| plugin.name == "stays-on")
            .unwrap_or_else(|| panic!("enabled plugin missing from {listed:?}"));
        assert!(on.enabled, "enabled plugin reported as disabled");

        let off = listed
            .iter()
            .find(|plugin| plugin.name == "turned-off")
            .unwrap_or_else(|| panic!("disabled plugin missing from {listed:?}"));
        assert!(!off.enabled, "disabled plugin reported as enabled");
        assert_eq!(
            off.version.as_deref(),
            Some("1.0.0"),
            "version did not survive disabling: {off:?}"
        );
        assert!(
            off.granted.iter().any(|grant| grant == "log"),
            "grants did not survive disabling: {off:?}"
        );
    }

    // A name from the frontend must not escape either root: the toggle reuses
    // `ensure_safe_name`, exactly as remove does.
    #[test]
    fn set_plugin_enabled_rejects_unsafe_names() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        fs::create_dir_all(&plugins_dir).unwrap();
        fs::create_dir_all(&disabled_dir).unwrap();
        let sentinel = dir(&root, "keep.txt");
        fs::write(&sentinel, b"keep").unwrap();

        for evil in [
            "",
            ".",
            "..",
            "../keep.txt",
            "../..",
            "nested/child",
            "back\\slash",
        ] {
            for enabled in [true, false] {
                let err = move_plugin_between_roots(&plugins_dir, &disabled_dir, evil, enabled)
                    .expect_err("unsafe name rejected");
                assert!(
                    matches!(err, PluginError::Invalid { .. }),
                    "{evil:?} (enabled={enabled}): {err:?}"
                );
            }
        }
        assert!(sentinel.is_file(), "sentinel outside both roots survived");
        assert!(
            plugins_dir.is_dir() && disabled_dir.is_dir(),
            "roots survived"
        );
    }

    #[test]
    fn set_plugin_enabled_is_idempotent() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        write_plugin(&plugins_dir, "steady");

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "steady", true)
            .expect("enabling an already-enabled plugin succeeds");
        assert!(plugins_dir.join("steady/plugin.toml").is_file());
        assert!(
            !disabled_dir.exists(),
            "enabling an enabled plugin touched the disabled root"
        );

        move_plugin_between_roots(&plugins_dir, &disabled_dir, "steady", false)
            .expect("first disable");
        move_plugin_between_roots(&plugins_dir, &disabled_dir, "steady", false)
            .expect("disabling an already-disabled plugin succeeds");
        assert!(disabled_dir.join("steady/plugin.toml").is_file());
        assert!(!plugins_dir.join("steady").exists());
    }

    // ADR-0034 consequence: two roots now hold plugin directories, so uninstall
    // must delete from whichever one holds the plugin.
    #[test]
    fn remove_plugin_deletes_from_either_root() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let disabled_dir = dir(&root, "plugins-disabled");
        write_plugin(&plugins_dir, "enabled-one");
        write_plugin(&disabled_dir, "disabled-one");

        remove_plugin_from_roots(&plugins_dir, &disabled_dir, "enabled-one")
            .expect("removes from the scanned root");
        assert!(!plugins_dir.join("enabled-one").exists());

        remove_plugin_from_roots(&plugins_dir, &disabled_dir, "disabled-one")
            .expect("removes from the disabled root");
        assert!(
            !disabled_dir.join("disabled-one").exists(),
            "uninstalling a disabled plugin silently did nothing"
        );

        let err = remove_plugin_from_roots(&plugins_dir, &disabled_dir, "never-installed")
            .expect_err("an unknown plugin is still an error");
        assert!(matches!(err, PluginError::Invalid { .. }), "got {err:?}");
    }

    // Regression: list_via_host must not pass args the host rejects. The host
    // only accepts --config/--plugins, so a stray `--contract report` made it
    // exit before serving and every list/add call failed.
    #[test]
    fn list_via_host_lists_an_installed_plugin() {
        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        write_loadable_plugin(&plugins_dir, "txt-importer");
        let config = write_host_config(&root);

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
        let source = write_loadable_plugin(&dir(&root, "src"), "rollback-me");
        let plugins_dir = dir(&root, "plugins");
        let bogus_host = dir(&root, "does-not-exist-host");
        let config = write_host_config(&root);

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
        let outside = dir(&root, "secret.txt");
        fs::write(&outside, b"secret").unwrap();
        let source = write_plugin(&dir(&root, "src"), "sneaky");
        std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
        let plugins_dir = dir(&root, "plugins");

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
        let plugins_dir = dir(&root, "plugins");
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

        let config = dir(&root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"xml\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary().into_std_path_buf())
            .config(config)
            .plugins(&plugins_dir);
        let mut remote = stanchion::remote::RemoteRegistry::launch(options).expect("host launches");
        let out: serde_json::Value = remote
            .call(
                "xml-probe",
                "root",
                [serde_json::json!(
                    "<package><metadata id=\"m1\"/></package>"
                )],
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
        let db_path = dir(&root, "books.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE books(id INTEGER, title TEXT);\
                 INSERT INTO books VALUES (1, 'Dune'), (2, 'Neuromancer');",
            )
            .unwrap();
        }
        let other_db = dir(&root, "secret.db");
        rusqlite::Connection::open(&other_db).unwrap();

        let plugins_dir = dir(&root, "plugins");
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

        let config = dir(&root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"sqlite\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary().into_std_path_buf())
            .config(config)
            .plugins(&plugins_dir)
            .arg("--sqlite")
            .arg(db_path.as_os_str());
        let mut remote = stanchion::remote::RemoteRegistry::launch(options).expect("host launches");

        let titles: serde_json::Value = remote
            .call(
                "sqlite-probe",
                "titles",
                [serde_json::json!(db_path.as_str())],
            )
            .expect("query the granted database");
        assert_eq!(titles, serde_json::json!("Dune,Neuromancer"));

        let allowed: serde_json::Value = remote
            .call(
                "sqlite-probe",
                "forbidden",
                [serde_json::json!(other_db.as_str())],
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
        let plugins_dir = dir(&root, "plugins");
        let plugin_dir = plugins_dir.join("http-probe");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"http-probe\"\nversion = \"1.0.0\"\n\n[capabilities.http]\ndomains = [\"127.0.0.1\"]\n",
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

        let config = dir(&root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\ncallbacks = [\"http\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let callback_dir = plugins_dir.clone();
        let options = stanchion::remote::RemoteOptions::new(host_binary().into_std_path_buf())
            .config(config)
            .plugins(&plugins_dir);
        let remote = stanchion::remote::RemoteRegistry::launch(options).expect("host launches");
        let mut remote = remote.on_callback(move |call| {
            // The allowlist is the app's own read of the manifest intersected with
            // the grant the host forwards with the call.
            crate::commands::plugin_host_callbacks::answer_http(call, &callback_dir)
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

    /// A manifest that grants `http` but names no domains reaches nothing: the
    /// host forwards the empty grant, and the app refuses before any socket is
    /// opened.
    #[test]
    fn http_capability_without_granted_domains_is_refused() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let root = tempfile::tempdir().unwrap();
        let plugins_dir = dir(&root, "plugins");
        let plugin_dir = plugins_dir.join("http-denied");
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            "name = \"http-denied\"\nversion = \"1.0.0\"\n\n[capabilities.http]\n",
        )
        .unwrap();
        fs::write(
            plugin_dir.join("init.lua"),
            r#"
local P = {}
P.__index = P
function P.new(config, deps) return setmetatable({}, P) end
function P:fetch(url)
  local ok, err = pcall(function() return http({ url = url }) end)
  if not ok then return "denied: " .. tostring(err) end
  return "allowed"
end
return P
"#,
        )
        .unwrap();

        let config = dir(&root, "host.toml");
        fs::write(
            &config,
            "[capabilities]\ncallbacks = [\"http\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();

        let options = stanchion::remote::RemoteOptions::new(host_binary().into_std_path_buf())
            .config(config)
            .plugins(&plugins_dir);
        let remote = stanchion::remote::RemoteRegistry::launch(options).expect("host launches");
        let callback_dir = plugins_dir.clone();
        let mut remote = remote.on_callback(move |call| {
            crate::commands::plugin_host_callbacks::answer_http(call, &callback_dir)
        });

        let out: serde_json::Value = remote
            .call(
                "http-denied",
                "fetch",
                [serde_json::json!(format!("http://{addr}/"))],
            )
            .expect("the plugin catches its own refusal");

        let message = out.as_str().expect("the plugin returns the refusal text");
        assert!(
            message.contains("manifest grants no domains"),
            "an ungranted http call must be refused, got `{message}`"
        );
        assert!(
            listener.accept().is_err(),
            "a refused call must not reach the socket"
        );
    }
}
