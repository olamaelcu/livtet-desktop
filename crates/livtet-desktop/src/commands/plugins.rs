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
        .plugins(&plugins_dir)
        .arg("--contract")
        .arg("report");
    let mut registry =
        stanchion::remote::RemoteRegistry::launch(options).map_err(PluginError::host)?;
    let plugins = registry.list().map_err(PluginError::host)?;
    Ok(plugins.into_iter().map(PluginSummary::from).collect())
}

/// Rejects names that are not a single, safe path segment, so a plugin name from
/// a manifest or the frontend can never escape the plugins directory.
fn ensure_safe_name(name: &str) -> Result<(), PluginError> {
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == "." || name == ".." {
        return Err(PluginError::invalid(format!("unsafe plugin name: {name:?}")));
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
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
    copy_dir_recursive(source, &dest).map_err(PluginError::host)?;
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

/// Installs a plugin from a local directory (developer / manual sideload), then
/// confirms the host can load it and returns its summary. If the copied plugin
/// fails to load, the install is rolled back so the plugins directory is never
/// left with a broken plugin.
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

    tokio::task::spawn_blocking(move || {
        let name = install_plugin_dir(&plugins_dir, &source)?;
        let loaded = list_via_host(host, config, plugins_dir.clone())?;
        match loaded.into_iter().find(|plugin| plugin.name == name) {
            Some(summary) => Ok(summary),
            None => {
                let _ = std::fs::remove_dir_all(plugins_dir.join(&name));
                Err(PluginError::invalid(format!(
                    "plugin \"{name}\" was installed but the host could not load it"
                )))
            }
        }
    })
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

    use super::{PluginError, install_plugin_dir, remove_plugin_dir};

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
            assert!(matches!(err, PluginError::Invalid { .. }), "{evil}: {err:?}");
        }
        assert!(sentinel.is_file(), "sentinel outside plugins dir survived");
    }
}
