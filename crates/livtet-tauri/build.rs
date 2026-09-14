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
    let env_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.mise/secrets.env");

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

    // ── Sidecar binary (livtet-plugins-host-lua) ──────────────────
    // Find the git checkout path (Cargo already fetched it as a dep)
    // and build the binary from there using its own target directory.
    let meta_out = std::process::Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .output()
        .expect("cargo metadata");
    let meta: serde_json::Value =
        serde_json::from_slice(&meta_out.stdout).expect("parse cargo metadata");
    let manifest_path = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "livtet-plugins")
        .and_then(|p| p["manifest_path"].as_str())
        .expect("livtet-plugins manifest_path");
    let plugins_dir = Path::new(manifest_path).parent().unwrap();

    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let cargo_profile = if profile == "debug" { "dev" } else { &profile };
    let sidecar_target = Path::new(env!("CARGO_MANIFEST_DIR")).join("target");

    let build_status = std::process::Command::new("cargo")
        .args([
            "build",
            "--bin",
            "livtet-plugins-host-lua",
            "--profile",
            cargo_profile,
            "--target-dir",
        ])
        .arg(&sidecar_target)
        .current_dir(plugins_dir)
        .env_remove("RUSTC_WRAPPER")
        .status()
        .expect("cargo build sidecar");

    if !build_status.success() {
        eprintln!("error: sidecar build failed (profile={profile})");
        std::process::exit(1);
    }

    let sidecar_bin = sidecar_target
        .join(&profile)
        .join("livtet-plugins-host-lua");

    let binaries_dir = Path::new("binaries");
    let target_triple = std::env::var("TARGET")
        .unwrap_or_else(|_| "x86_64-unknown-linux-gnu".into());
    std::fs::create_dir_all(binaries_dir.join("bin"))
        .expect("create binaries/bin dir");
    std::fs::copy(
        &sidecar_bin,
        binaries_dir
            .join("bin")
            .join(format!("livtet-plugins-host-lua-{target_triple}")),
    )
    .expect("copy sidecar binary");

    tauri_build::build();
}
