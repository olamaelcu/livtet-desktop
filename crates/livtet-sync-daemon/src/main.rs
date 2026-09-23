//! Thin sidecar wrapper around `livtet-sync-server`.
//!
//! Started by the desktop app through Tauri's managed-sidecar mechanism, this
//! binary just parses CLI arguments, initializes logging on stderr, and hands
//! off to [`livtet_sync_server::run`]. Stdout is intentionally left untouched:
//! it belongs to the daemon's newline-delimited JSON-RPC channel.

use std::process::ExitCode;

use livtet_sync_server::{ServerConfig, run};

const USAGE: &str = "\
livtet-sync-daemon — Livtet sync server sidecar

USAGE:
    livtet-sync-daemon [OPTIONS]

OPTIONS:
    --db <PATH>        SQLite database path (default: livtet-sync.db)
    --host <HOST>      Interface to bind the sync HTTP server on (default: 127.0.0.1)
    --port <PORT>      Port to bind; 0 selects an ephemeral port (default: 0)
    --device-id <ID>   Pre-set device id (default: read from the database)
    -h, --help         Print this help message and exit
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        // Help is the one thing allowed to speak on stdout.
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    init_tracing();

    let config = match ServerConfig::from_args(args.into_iter()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("livtet-sync-daemon: {error}");
            eprintln!("Try 'livtet-sync-daemon --help' for usage.");
            return ExitCode::from(2);
        }
    };

    tracing::info!(
        db = %config.db_path,
        host = %config.host,
        port = config.port,
        "livtet-sync-daemon starting"
    );

    match run(config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("livtet-sync-daemon: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Install a stderr-only tracing subscriber honoring `RUST_LOG`.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
