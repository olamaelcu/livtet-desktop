use std::collections::HashMap;
use std::path::Path;

use config::{Config, Environment};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Secrets {
    google_books_api_key: String,
    sentry_dsn: String,
}

fn parse_env_file(path: &Path) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(content) = std::fs::read_to_string(path) else {
        return map;
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim().to_string();
        let value = v.trim().trim_matches('\'').trim_matches('"').to_string();
        map.insert(key, value);
    }
    map
}

fn main() {
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.mise/secrets.env");

    println!("cargo:rerun-if-changed={}", env_path.display());
    println!("cargo:rerun-if-env-changed=GOOGLE_BOOKS_API_KEY");
    println!("cargo:rerun-if-env-changed=SENTRY_DSN");

    if !env_path.exists() {
        eprintln!(
            "error: '{}' is missing.\n\
             Run: 'mise run secrets-decrypt' && 'mise run secrets-export-env'\n
            ",
            env_path.display()
        );
        std::process::exit(1);
    }

    let file_map = parse_env_file(&env_path);
    let env_map: HashMap<String, String> = std::env::vars().collect();

    let config = Config::builder()
        .add_source(Environment::default().source(Some(file_map)))
        .add_source(Environment::default().source(Some(env_map)))
        .build()
        .expect("failed to build secrets config");

    let secrets: Secrets = config
        .try_deserialize()
        .expect("missing required secrets in '.mise/secrets.env' (or env override)");

    if secrets.google_books_api_key.trim().is_empty() {
        eprintln!("error: GOOGLE_BOOKS_API_KEY is empty in .mise/secrets.env");
        std::process::exit(1);
    }
    if secrets.sentry_dsn.trim().is_empty() {
        eprintln!("error: SENTRY_DSN is empty in .mise/secrets.env");
        std::process::exit(1);
    }

    println!(
        "cargo:rustc-env=GOOGLE_BOOKS_API_KEY={}",
        secrets.google_books_api_key
    );
    println!("cargo:rustc-env=SENTRY_DSN={}", secrets.sentry_dsn);

    // ── Sync daemon sidecar (`livtet-sync-daemon`) ──────────────
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = manifest_dir
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root");
    let target_triple =
        std::env::var("TARGET").unwrap_or_else(|_| "x86_64-unknown-linux-gnu".into());
    let is_windows = target_triple.contains("windows");

    println!(
        "cargo:rerun-if-changed={}",
        workspace_dir.join("crates/livtet-sync-daemon").display()
    );

    // Build the daemon with its own target dir so the nested build does not
    // contend with this one, then stage it for Tauri (`externalBin`) and for
    // dev runs next to the application binary. `Command::status()` inherits
    // stdio (no pipes), which avoids the deadlock a JSON-parsing wrapper hits
    // when the child keeps writing after the artifact is found.
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let cargo_profile = if profile == "debug" {
        "dev"
    } else {
        profile.as_str()
    };
    let sidecar_target = manifest_dir.join("target").join("sync-daemon");

    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "-p",
            "livtet-sync-daemon",
            "--bin",
            "livtet-sync-daemon",
            "--profile",
            cargo_profile,
            "--target-dir",
        ])
        .arg(&sidecar_target)
        .current_dir(workspace_dir)
        .env_remove("RUSTC_WRAPPER")
        .status()
        .expect("build livtet-sync-daemon sidecar");

    if !status.success() {
        eprintln!("error: livtet-sync-daemon sidecar build failed (profile={profile})");
        std::process::exit(1);
    }

    let built = sidecar_target.join(&profile).join(if is_windows {
        "livtet-sync-daemon.exe"
    } else {
        "livtet-sync-daemon"
    });

    let binaries_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&binaries_dir).expect("create binaries dir");
    // Tauri expects `<name>-<target-triple>` (with a trailing `.exe` on Windows)
    // next to the `externalBin` entry in `tauri.conf.json`.
    let staged_name = if is_windows {
        format!("livtet-sync-daemon-{target_triple}.exe")
    } else {
        format!("livtet-sync-daemon-{target_triple}")
    };
    std::fs::copy(&built, binaries_dir.join(staged_name)).expect("stage sync daemon for bundling");

    // Dev runs resolve the sidecar next to the application binary.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let mut profile_dir = Path::new(&out_dir).to_path_buf();
        for _ in 0..3 {
            if let Some(parent) = profile_dir.parent() {
                profile_dir = parent.to_path_buf();
            }
        }
        let bin_name = if is_windows {
            "livtet-sync-daemon.exe"
        } else {
            "livtet-sync-daemon"
        };
        let _ = std::fs::copy(&built, profile_dir.join(bin_name));
    }

    tauri_build::build();
}
