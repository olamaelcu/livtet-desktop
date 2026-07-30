use std::path::Path;

use config::{Config, Environment, File, FileFormat};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Secrets {
    google_books_api_key: String,
    sentry_dsn:           String,
}

fn main() {
    println!("cargo:rerun-if-changed=.mise/secrets.env");
    println!("cargo:rerun-if-env-changed=GOOGLE_BOOKS_API_KEY");
    println!("cargo:rerun-if-env-changed=SENTRY_DSN");

    let env_path = Path::new(".mise/secrets.env");
    if !env_path.exists() {
        eprintln!(
            "error: .mise/secrets.env is missing.\n\
             Run: mise run secrets-decrypt && mise run secrets-export-env\n\
             (first time:  mise run secrets-init)"
        );
        std::process::exit(1);
    }

    let config = Config::builder()
        .add_source(File::new(".mise/secrets.env", FileFormat::Env))
        .add_source(Environment::default().separator("_"))
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