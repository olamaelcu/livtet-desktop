//! Persisted sync preferences.
//!
//! The chosen device name is read during app setup so the daemon is spawned
//! with `--device-name`, and saving it later applies live through the
//! `discovery.rename` RPC. Keeping it in its own store mirrors the MCP and
//! OPDS server settings stores.
//!
//! The listen port lives here for the same reason the name does — it travels
//! as a spawn flag — but it is remembered rather than chosen: a paired peer
//! stores this device's address once, at pairing time, so the server has to
//! come back on the same port for that pairing to keep working.

use serde::{Deserialize, Serialize};

/// The settings stored in `sync.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSettings {
    /// Name advertised to nearby peers; absent/blank generates one from the
    /// device id.
    #[serde(default)]
    pub device_name: Option<String>,
    /// Bind the sync daemon on all interfaces so peers on the LAN can reach
    /// it. Defaults to `true`: `/sync/*` data routes require a session token,
    /// and pairing still needs approval here. Set `false` to keep the daemon
    /// on loopback.
    #[serde(default = "lan_enabled")]
    pub lan: bool,
    /// Port the sync server binds, remembered from the last successful start.
    ///
    /// `None` on a fresh install (and on settings written before this field
    /// existed); the first start records what the daemon actually bound.
    #[serde(default)]
    pub listen_port: Option<u16>,
}

/// Default for [`SyncSettings::lan`], applied to settings written before the
/// field existed.
fn lan_enabled() -> bool {
    true
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            device_name: None,
            lan: lan_enabled(),
            listen_port: None,
        }
    }
}

impl SyncSettings {
    /// Replace the advertised name while keeping every other preference.
    pub fn with_device_name(mut self, name: Option<String>) -> Self {
        self.device_name = name;
        self
    }

    /// Remember the port the daemon bound while keeping every other
    /// preference.
    pub fn with_listen_port(mut self, port: Option<u16>) -> Self {
        self.listen_port = port;
        self
    }
}

/// Key the settings are stored under inside `sync.json`.
pub const SETTINGS_KEY: &str = "sync";

/// Storage seam so fixtures need no Tauri store.
pub trait SyncSettingsStore: Send + Sync {
    /// The persisted settings, absent until the first save.
    fn read(&self) -> Option<SyncSettings>;
    /// Replace the persisted settings and flush them.
    fn write(&self, settings: &SyncSettings) -> Result<(), String>;
}

impl SyncSettingsStore for tauri_plugin_store::Store<tauri::Wry> {
    fn read(&self) -> Option<SyncSettings> {
        self.get(SETTINGS_KEY)
            .and_then(|value| serde_json::from_value(value).ok())
    }

    fn write(&self, settings: &SyncSettings) -> Result<(), String> {
        let value = serde_json::to_value(settings).map_err(|error| error.to_string())?;
        self.set(SETTINGS_KEY, value);
        self.save().map_err(|error| error.to_string())
    }
}

/// Trim a chosen name. An empty field means "use the generated name", which
/// the daemon models as `None`.
pub fn normalize_device_name(name: Option<String>) -> Option<String> {
    name.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::normalize_device_name;

    #[test]
    fn a_blank_name_means_generate_one() {
        assert_eq!(normalize_device_name(None), None);
        assert_eq!(normalize_device_name(Some(String::new())), None);
        assert_eq!(normalize_device_name(Some("   ".to_string())), None);
    }

    #[test]
    fn a_chosen_name_is_trimmed() {
        assert_eq!(
            normalize_device_name(Some("  Josh's MacBook  ".to_string())).as_deref(),
            Some("Josh's MacBook")
        );
    }

    #[test]
    fn renaming_keeps_the_other_preferences() {
        use super::SyncSettings;

        let offline = SyncSettings {
            device_name: None,
            lan: false,
            listen_port: Some(7723),
        };
        let renamed = offline.with_device_name(Some("Josh's MacBook".to_string()));

        assert_eq!(renamed.device_name.as_deref(), Some("Josh's MacBook"));
        assert!(!renamed.lan, "a rename must not turn LAN sharing back on");
        assert_eq!(
            renamed.listen_port,
            Some(7723),
            "a rename must not strand the address every peer already stores"
        );
    }

    /// The remembered port is only useful if it survives the store: a settings
    /// file written before the field existed must load, and the port must come
    /// back on the next launch.
    #[test]
    fn the_listen_port_survives_the_store_and_older_settings_files() {
        use super::SyncSettings;

        let legacy: SyncSettings = serde_json::from_value(serde_json::json!({
            "device_name": "Josh's MacBook",
            "lan": true,
        }))
        .expect("settings written before the field existed still load");
        assert_eq!(legacy.listen_port, None);

        let stored = serde_json::to_value(legacy.clone().with_listen_port(Some(7740)))
            .expect("settings serialize");
        let reloaded: SyncSettings = serde_json::from_value(stored).expect("settings round-trip");

        assert_eq!(reloaded.listen_port, Some(7740));
        assert_eq!(reloaded, legacy.with_listen_port(Some(7740)));
    }
}
