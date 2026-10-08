//! Locating the plugin host sidecar binary.

use camino::Utf8PathBuf;

const PLUGIN_HOST_BIN: &str = if cfg!(windows) {
    "livtet-plugin-host.exe"
} else {
    "livtet-plugin-host"
};

/// Resolves the plugin host binary: an explicit override first, then next to the
/// application binary (dev and bundled installs), then the application data dir.
pub(crate) fn resolve_plugin_host(app_dir: &Utf8PathBuf) -> Utf8PathBuf {
    if let Some(path) = std::env::var_os("LIVTET_PLUGIN_HOST") {
        return Utf8PathBuf::from(path.to_string_lossy().into_owned());
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(PLUGIN_HOST_BIN);
        if candidate.is_file() {
            return Utf8PathBuf::from_path_buf(candidate)
                .unwrap_or_else(|path| Utf8PathBuf::from(path.to_string_lossy().into_owned()));
        }
    }

    app_dir.join(PLUGIN_HOST_BIN)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// All three behaviours live in one test: `LIVTET_PLUGIN_HOST` is
    /// process-global, so splitting them would let parallel tests clobber each
    /// other's environment.
    #[test]
    fn resolves_the_env_override_before_anything_else() {
        // SAFETY: edition 2024 marks env mutation unsafe. This test owns the
        // variable for its duration and clears it before returning; no other
        // test in this crate reads it.
        unsafe { std::env::remove_var("LIVTET_PLUGIN_HOST") };

        // Without an override the binary name is always the last component,
        // whether it resolved next to the executable or under the app dir.
        let fallback = resolve_plugin_host(&Utf8PathBuf::from("/app"));
        assert_eq!(fallback.file_name(), Some(PLUGIN_HOST_BIN));

        // An override is returned verbatim, with no check that it exists.
        let missing = "/nowhere/at/all/livtet-plugin-host";
        unsafe { std::env::set_var("LIVTET_PLUGIN_HOST", missing) };
        assert_eq!(
            resolve_plugin_host(&Utf8PathBuf::from("/app")),
            Utf8PathBuf::from(missing)
        );

        unsafe { std::env::remove_var("LIVTET_PLUGIN_HOST") };
    }
}
