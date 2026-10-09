//! Locating the plugin host sidecar binary.

use std::ffi::OsStr;
use std::path::Path;

use camino::Utf8PathBuf;

const PLUGIN_HOST_BIN: &str = if cfg!(windows) {
    "livtet-plugin-host.exe"
} else {
    "livtet-plugin-host"
};

/// Resolves the plugin host binary: an explicit override first, then next to the
/// application binary (dev and bundled installs), then the application data dir.
///
/// Reads the environment and the running executable, then defers to
/// [`resolve_plugin_host_with`] for the decision itself.
pub(crate) fn resolve_plugin_host(app_dir: &Utf8PathBuf) -> Utf8PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));

    resolve_plugin_host_with(
        std::env::var_os("LIVTET_PLUGIN_HOST").as_deref(),
        exe_dir.as_deref(),
        app_dir,
    )
}

/// The resolution order itself, with its two ambient inputs passed in.
///
/// Separate from [`resolve_plugin_host`] so tests can drive every branch
/// without mutating the process environment: `std::env::set_var` is unsafe in
/// edition 2024 because a concurrent `getenv` elsewhere in the same test binary
/// can observe a reallocated `environ`.
fn resolve_plugin_host_with(
    override_path: Option<&OsStr>,
    exe_dir: Option<&Path>,
    app_dir: &Utf8PathBuf,
) -> Utf8PathBuf {
    if let Some(path) = override_path {
        return Utf8PathBuf::from(path.to_string_lossy().into_owned());
    }

    if let Some(dir) = exe_dir {
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
    use std::ffi::OsString;

    /// Tests drive the pure core, so nothing mutates the process environment:
    /// `std::env::set_var` is unsafe in edition 2024 because a concurrent
    /// `getenv` anywhere in the test binary can tear. Several tests in
    /// `commands::plugins` call `tempfile::tempdir()`, which reads TMPDIR.
    fn resolve(
        override_path: Option<&str>,
        exe_dir: Option<&std::path::Path>,
        app_dir: &str,
    ) -> Utf8PathBuf {
        let owned = override_path.map(OsString::from);
        resolve_plugin_host_with(owned.as_deref(), exe_dir, &Utf8PathBuf::from(app_dir))
    }

    #[test]
    fn an_override_wins_over_every_other_source() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(PLUGIN_HOST_BIN), b"#!/bin/sh\n").unwrap();

        let resolved = resolve(Some("/opt/custom/host"), Some(dir.path()), "/app");

        assert_eq!(resolved, Utf8PathBuf::from("/opt/custom/host"));
    }

    /// The override is trusted as given; nothing checks that it exists.
    #[test]
    fn an_override_is_returned_without_an_existence_check() {
        let resolved = resolve(Some("/nowhere/at/all/livtet-plugin-host"), None, "/app");

        assert_eq!(
            resolved,
            Utf8PathBuf::from("/nowhere/at/all/livtet-plugin-host")
        );
    }

    #[test]
    fn a_sibling_of_the_executable_is_preferred_when_present() {
        let dir = tempfile::tempdir().unwrap();
        let sibling = dir.path().join(PLUGIN_HOST_BIN);
        std::fs::write(&sibling, b"#!/bin/sh\n").unwrap();

        let resolved = resolve(None, Some(dir.path()), "/app");

        assert_eq!(resolved.as_std_path(), sibling);
    }

    /// The documented last resort, asserted as a whole path rather than a
    /// basename: a version that ignored `app_dir` would pass a basename check.
    #[test]
    fn falls_back_to_the_app_dir_when_no_sibling_exists() {
        let empty = tempfile::tempdir().unwrap();

        let resolved = resolve(None, Some(empty.path()), "/app");

        assert_eq!(resolved, Utf8PathBuf::from("/app").join(PLUGIN_HOST_BIN));
    }

    #[test]
    fn falls_back_to_the_app_dir_when_the_executable_is_unknown() {
        let resolved = resolve(None, None, "/app");

        assert_eq!(resolved, Utf8PathBuf::from("/app").join(PLUGIN_HOST_BIN));
    }
}
