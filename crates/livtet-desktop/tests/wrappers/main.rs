//! Command-wrapper coverage against a mocked Tauri app.
//!
//! These five suites were separate integration binaries, so every run statically
//! linked the whole Tauri graph once per suite (plus once for `live_opds_smoke`).
//! On Windows that linking dominates: 16 minutes of Rust work against 4.7 minutes
//! of actual test execution. One binary keeps the same tests and the same
//! isolation between modules, but links the graph once.
//!
//! The shared mock scaffolding already lives in the crate (`test_fixture`), so
//! modules only need to be reachable here; each module keeps its own
//! `#![allow(clippy::unwrap_used, clippy::expect_used)]` header.
//!
//! Test names gain the module as a prefix
//! (`wrappers::mock_reader::…`), so `cargo nextest -E 'test(/mock_reader/)'` and
//! `cargo test wrappers::mock_reader` still select a single suite.

mod mock_bulk;
mod mock_import_search;
mod mock_opds_enrich;
mod mock_plugin_sync;
mod mock_reader;

/// `#[ignore]`d live smoke test that needs the desktop app running. It lives here
/// so it does not cost another link of the Tauri graph on every run; select it
/// with `mise run smoke:mcp`.
mod live_opds_smoke;
