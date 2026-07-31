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

The items below were the original list.  Items marked **Done (2026-07-31)**
were implemented in a single pass; the others remain as future work.

* **~~Free use of `#[tracing::instrument]` on Tauri commands and async tasks.~~**
  **Done (2026-07-31).**  All 14 Tauri commands now carry
  `#[tracing::instrument(skip(state), err)]`.  Command-related
  spans are recorded by name; ad-hoc `fields(...)` with display
  sigils were not used because the proc-macro parser in the
  edition-2024 crate rejects them (the sigil parses as an operator
  inside the attribute).  Call-sites that need per-invocation
  fields add `tracing::info!/debug!` calls manually.

* **Per-subsystem rolling files.**  Infrastructure exists:
  `SubsystemLogger { name, filter }` is defined in `lib.rs` and
  the `init_tracing` signature accepts `&[SubsystemLogger]`, but
  the loop body is a no-op today because `fmt::Layer::with_filter`
  requires a static subscriber type and the subsystem layers
  cannot be composed dynamically onto the same `Registry` chain.
  Callers should create their own `fmt::layer()` with writer and
  filter in their own init function; the `SubsystemLogger` type
  serves as a structural contract.

* **~~Structured / JSON logs for shippers.~~**  **Done (2026-07-31).**
  Set `LIVTET_LOG_FORMAT=json` in the environment to swap the file
  layer from plain `fmt::layer()` to `fmt::layer().json()`.
  Requires `tracing-subscriber` feature `json` (added to
  `tauri/Cargo.toml`).

* **~~In-app log export.~~**  **Done (2026-07-31).**
  `commands::diagnostics::export_logs` reads all `.log` files from
  `logs_dir` (now stored in `AppState`) and returns
  `Vec<LogFile>` (filename + content strings).  Uses
  `tokio::task::spawn_blocking` with `std::fs::read_dir` to avoid
  pulling in the `tokio` `fs` feature.  No compression yet; log
  files are small enough to send as text.

* **~~Sampling / rate limits.~~**  **Done (2026-07-31).**
  `logging_rate_limit.rs` provides per-target token-bucket rate
  limiting via env var:
  `LIVTET_LOG_RATE_LIMIT=remote_search=10/s,import_edition=1/s`.
  Implemented as a function-pointer `FilterFn` (closures cannot
  capture state with `FilterFn::new`; the token buckets live in
  `LazyLock<Mutex<HashMap<…>>>` statics).  Warnings and errors
  always pass through regardless of rate limit.  The filter is
  placed before `ForestLayer` and the file layer in the subscriber
  chain.

* **~~Crash on panic.~~**  **Done (2026-07-31).**
  `init_tracing` sets a panic hook that logs the panic message
  via `tracing::error!`, drops the `WorkerGuard` (triggering the
  non-blocking writer's flush), sleeps 100ms, then calls the
  previous hook + exit.  `LOG_FILE_GUARD` was changed from
  `OnceLock<WorkerGuard>` to `Mutex<Option<WorkerGuard>>` to
  allow the guard to be taken and dropped in the hook.

* **~~Test-mode switching.~~**  **Done (2026-07-31).**
  `init_test_tracing()` (gated on `#[cfg(test)]` in `lib.rs`)
  initialises `ForestLayer::default()` at `warn` level (or
  `RUST_LOG` if set).  Does not create a file layer, so tests
  never write into the user's real `logs_dir`.  Test modules that
  want trace output can call this in their test setup.