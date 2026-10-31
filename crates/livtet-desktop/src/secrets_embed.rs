//! Build-time embedded secrets from `.mise/tasks/secrets-export-env` → `build.rs`.
//! These are baked into the binary via `rustc-env` and are always available
//! at runtime regardless of whether `.mise/secrets.env` is installed.

pub const GOOGLE_BOOKS_API_KEY: &str = env!("GOOGLE_BOOKS_API_KEY");
pub const SENTRY_DSN: &str = env!("SENTRY_DSN");
