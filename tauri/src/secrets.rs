//! Compile-time secrets embedded from `.mise/secrets.env` by `tauri/build.rs`.
//!
//! Each value is a `pub const &str` baked into the binary at build time
//! via `cargo:rustc-env`. To rotate, run:
//!
//! ```text
//! mise run secrets-edit           # edit the encrypted JSON
//! mise run secrets-decrypt        # refresh .mise/secrets.json
//! mise run secrets-export-env     # refresh .mise/secrets.env
//! cargo build                     # rebuild the binary
//! ```
//!
//! Override any value for a one-off build with an env var, e.g.:
//!
//! ```text
//! GOOGLE_BOOKS_API_KEY=dev-test cargo build
//! ```

pub const GOOGLE_BOOKS_API_KEY: &str = env!("GOOGLE_BOOKS_API_KEY");
pub const SENTRY_DSN: &str           = env!("SENTRY_DSN");
