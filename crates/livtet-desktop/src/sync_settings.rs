//! Persisted sync preferences.
//!
//! The chosen device name is read during app setup so the daemon is spawned
//! with `--device-name`, and saving it later applies live through the
//! `discovery.rename` RPC. Keeping it in its own store mirrors the MCP and
//! OPDS server settings stores.

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
        }
    }
}

impl SyncSettings {
    /// Replace the advertised name while keeping every other preference.
    pub fn with_device_name(mut self, name: Option<String>) -> Self {
        self.device_name = name;
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
        };
        let renamed = offline.with_device_name(Some("Josh's MacBook".to_string()));

        assert_eq!(renamed.device_name.as_deref(), Some("Josh's MacBook"));
        assert!(!renamed.lan, "a rename must not turn LAN sharing back on");
    }
}
