# 2. Adopt filtered tracing with rolling file output

Date: 2026-07-29

## Status

Accepted

## Context

The desktop app initialised logging with a single line inside `app_setup`:

```rust
tracing_forest::init();
```

That call installs `ForestLayer::default()` with no filter, so every event at
`TRACE` and above from every target reaches stdout. Two problems made this
unworkable in practice:

1. **Target-spam from a transitive dependency.** `tauri-plugin-mcp-bridge`
   pulls in `tokio-tungstenite 0.28`, whose `compat.rs` module emits a TRACE
   event on every poll of the underlying socket. Each session produced dozens
   of lines like:

   ```
   TRACE    📍 [trace]: .../tokio-tungstenite-0.28.0/src/compat.rs:157
            Read.with_context read -> poll_read
   ```

   These lines drowned out our own diagnostic output. We cannot fix the source
   crate, only filter its events.

2. **No persistent log history.** Everything went to stdout. After a crash or
   an unexplained shutdown there was nothing on disk to inspect. Debugging
   user-reported issues required capturing console output at the right moment.

The codebase had no project-local tracing setup yet, only the one-line
`tracing_forest::init()` and a `#[tracing::instrument]` annotation on
`AppDirectories::resolve` and `app_setup`. `tracing-subscriber` was already a
direct dependency but without the `env-filter` feature, so `EnvFilter` was
unavailable to us.

## Decision

Replaced the one-line `tracing_forest::init()` with a project-local
`init_tracing(logs_dir: &Utf8Path) -> miette::Result<()>` defined in
`tauri/src/lib.rs`, called from `app_setup` after the directory layout is
resolved.

### Subscriber composition

`init_tracing` builds a `tracing_subscriber::registry()` and layers, in order:

1. **Filter** — `EnvFilter` (the re-exported type from
   `tracing_forest::util::EnvFilter`, requiring `tracing-forest`'s
   `env-filter` feature). Defaults to `"info"`; `RUST_LOG` overrides. Two
   hardcoded directives are appended unconditionally so the noise stays muted
   regardless of verbosity:

   ```text
   tokio_tungstenite=off
   tokio_tungstenite::compat=off
   ```

2. **Stdout writer** — `ForestLayer::default()`. Preserves the existing
   `TRACE    📍 [trace]: path:line ...` tree output.

3. **File writer** — `fmt::layer().with_writer(non_blocking_file_writer)`
   with `with_ansi(false)`, `with_target(true)`, `with_file(true)`,
   `with_line_number(true)`. Renders a conventional line-oriented format
   suitable for grep and log shippers.

### Rolling file output

* `AppDirectories` gained a `logs_dir: Utf8PathBuf` field, derived as
  `<app_local_data_dir>/logs/`.
* `app_setup` calls `fs_err::tokio::create_dir_all(&paths.logs_dir)` and
  passes the path to `init_tracing`.
* `init_tracing` constructs `RollingFileAppender::new(Rotation::DAILY,
  logs_dir, "livtet.log")`, wraps it in `tracing_appender::non_blocking`,
  and stashes the resulting `WorkerGuard` in
  `static LOG_FILE_GUARD: OnceLock<WorkerGuard>` so the background writer
  thread outlives the init function.

### Dependencies

`tauri/Cargo.toml`:

* `tracing-forest` keeps its position; adds `features = ["env-filter"]` to
  unlock the `EnvFilter` re-export under `tracing_forest::util`.
* `tracing-subscriber` adds `features = ["env-filter"]` (also reachable
  transitively via `tracing-forest`; we declare it explicitly).
* `tracing-appender = "0.2"` is added for `RollingFileAppender` and
  `non_blocking`.

## Consequences

### What becomes easier

* **Per-target filtering without recompiling.** Setting `RUST_LOG=trace`
  enables full verbosity for everything except the muted `tokio_tungstenite`
  targets, which is what we want when debugging.
* **Persistent log history.** `<app_local_data_dir>/logs/livtet.log.YYYY-MM-DD`
  accumulates one file per UTC day. Files are conventional fmt, so `grep`,
  `tail -f`, and standard log tooling Just Work.
* **Diagnostic path for user-reported issues.** A user can be asked to attach
  the contents of `logs/livtet.log.<today>` instead of trying to capture a
  scrolling terminal.
* **The `tokio_tungstenite` mute is durable.** Hardcoded in `init_tracing`,
  not dependent on developer discipline to set `RUST_LOG`.

### What becomes harder

* **Two output formats in play.** Stdout uses ForestLayer's tree style; the
  file uses conventional `fmt`. Someone reading both side by side has to
  context-switch. Mitigation if it bothers us later: align file format with
  the tree style, or vice versa.
* **`WorkerGuard` lifetime.** Stored in `OnceLock` so it never drops until
  process exit. On hard crashes the OS reclaims the in-flight writes; we
  accept losing the last handful of lines.
* **Stdout and file layers cannot share state.** Each layer independently
  formats and writes; there is no built-in way to make the file output match
  the tree style without writing a custom Layer.

### What future implementation work can build on this

* **Free use of `#[tracing::instrument]` on Tauri commands and async tasks.**
  Filter directives can be added to `init_tracing` without touching any
  call sites. Future modules may declare their own target name (e.g.
  `livtet_core::sync`) and tune verbosity via `RUST_LOG=livtet_core=debug`.
* **Per-subsystem rolling files.** The same pattern (`RollingFileAppender` +
  `non_blocking` + `fmt::layer().with_writer(...)`) extends naturally.
  A future verbose-subsystem logger could write to
  `logs/<subsystem>/<name>.log.YYYY-MM-DD` by composing another `fmt::layer`
  onto the registry, gated by an additional `EnvFilter` with its own
  directive set.
* **Structured / JSON logs for shippers.** Swap the file `fmt::layer()` for
  `tracing_subscriber::fmt::layer().json()` to emit structured events. The
  registry composition stays the same; only the file-layer builder changes.
* **In-app log export.** A Tauri command can read `paths.logs_dir` (already
  available via `app.state::<…>()` once we publish it) and zip recent files
  for a "Send diagnostics" affordance. No new infrastructure needed.
* **Sampling / rate limits.** A custom Layer placed before `ForestLayer` and
  the file layer can drop or sample events for known-noisy targets without
  rewriting existing instrumentation.
* **Crash on panic.** `tracing-subscriber` has a panic hook that flushes the
  non-blocking writer before the process dies; enabling it would make sure
  the last events before a panic reach disk. One-line addition inside
  `init_tracing`.
* **Test-mode switching.** `tracing_forest::test_init()` could replace the
  registry during `cargo test` to avoid polluting the user's real log dir.
  Same `init_tracing` shape, swap the body via a cfg gate.