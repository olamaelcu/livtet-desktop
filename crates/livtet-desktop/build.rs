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
            "error: .mise/secrets.env is missing.\n\
             Run: mise run secrets-decrypt && mise run secrets-export-env\n\
             (first time:  mise run secrets-init)"
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
        .expect("missing required secrets in .mise/secrets.env (or env override)");

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

    // ── Plugin host sidecar (stanchion's `plugin-host`) ──────────────
    // The host is a binary of the `stanchion` dependency, which Cargo does not
    // build for us. Build it from its checkout with a target dir of its own so
    // the nested build does not contend with this one, then stage it for Tauri
    // (`externalBin`) and for dev runs next to the application binary.
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let cargo_profile = if profile == "debug" {
        "dev"
    } else {
        profile.as_str()
    };

    let metadata_output = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    let metadata: serde_json::Value =
        serde_json::from_slice(&metadata_output.stdout).expect("parse cargo metadata");
    let stanchion_manifest = metadata["packages"]
        .as_array()
        .expect("packages array")
        .iter()
        .find(|package| package["name"] == "stanchion")
        .and_then(|package| package["manifest_path"].as_str())
        .expect("stanchion is a dependency");
    let stanchion_dir = Path::new(stanchion_manifest)
        .parent()
        .expect("stanchion manifest dir");

    let sidecar_target = manifest_dir.join("target").join("plugin-host");
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "--bin",
            "plugin-host",
            "--features",
            "remote,lua54,vendored",
            "--profile",
            cargo_profile,
            "--target-dir",
        ])
        .arg(&sidecar_target)
        .current_dir(stanchion_dir)
        .env_remove("RUSTC_WRAPPER")
        .status()
        .expect("build plugin host sidecar");

    if !status.success() {
        eprintln!("error: plugin host sidecar build failed (profile={profile})");
        std::process::exit(1);
    }

    let target_triple =
        std::env::var("TARGET").unwrap_or_else(|_| "x86_64-unknown-linux-gnu".into());
    let is_windows = target_triple.contains("windows");
    let built = sidecar_target.join(&profile).join(if is_windows {
        "plugin-host.exe"
    } else {
        "plugin-host"
    });

    // Tauri's `externalBin` wants `<name>-<target-triple>` beside the config.
    let binaries_dir = manifest_dir.join("binaries");
    std::fs::create_dir_all(&binaries_dir).expect("create binaries dir");
    std::fs::copy(
        &built,
        binaries_dir.join(format!("livtet-plugin-host-{target_triple}")),
    )
    .expect("stage plugin host for bundling");

    // Dev runs resolve the host next to the application binary, without Tauri.
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let mut profile_dir = Path::new(&out_dir).to_path_buf();
        for _ in 0..3 {
            if let Some(parent) = profile_dir.parent() {
                profile_dir = parent.to_path_buf();
            }
        }
        let host_name = if is_windows {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        };
        let _ = std::fs::copy(&built, profile_dir.join(host_name));
    }

    tauri_build::build();
}
