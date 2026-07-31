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

    tauri_build::build();
}
