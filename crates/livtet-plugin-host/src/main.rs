//! Livtet's out-of-process plugin host.
//!
//! Runs Livtet's Lua plugins in a child process, speaking newline-delimited JSON
//! over stdio, so a plugin that loops forever, exhausts memory, or crashes the
//! interpreter takes down this process instead of the desktop app that launched
//! it. The desktop app resolves this binary as `livtet-plugin-host`
//! (`crates/livtet-desktop/src/lib.rs`, `resolve_plugin_host`) and drives it
//! through `stanchion::remote::RemoteRegistry`.
//!
//! ```text
//! livtet-plugin-host --config host.toml [--plugins DIR]
//! ```
//!
//! The host is contract-agnostic: it loads whatever plugins live under the
//! plugins root and answers method calls by name. The importer contract
//! (`extensions`, `read_metadata`) and the `report` listing are enforced by the
//! caller in `livtet-desktop`, not here. stdin and stdout carry JSON-RPC 2.0,
//! one message per line; stderr is free for logging.

use std::{
    io::{self, BufReader},
    process::ExitCode,
};

use camino::Utf8PathBuf;
use stanchion::remote::{HostChannel, build_registry, load_config, serve};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("livtet-plugin-host: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse(std::env::args().skip(1))?;
    if options.help {
        println!("{USAGE}");
        return Ok(());
    }

    let mut config = match &options.config {
        Some(path) => load_config(path.as_path().as_std_path())?,
        None => Default::default(),
    };
    if let Some(plugins) = options.plugins {
        config.plugins = Some(plugins.into_std_path_buf());
    }

    let channel = HostChannel::new(
        BufReader::new(io::stdin()),
        io::BufWriter::new(io::stdout()),
    );
    // `isolation` is the posture this host attests to; `serve` reports it.
    let (mut registry, isolation) = build_registry(&config, &channel)?;

    // Livtet's in-host module capabilities (xml, sqlite, json, base64) are gone:
    // a capability provider no longer receives a Lua handle, so the host cannot
    // build a module whose members are functions. Upstream:
    // https://github.com/jalcine/stanchion/issues/56 . Until then a plugin that
    // declares one gets `UnknownCapability`, and `--sqlite` is accepted and
    // ignored so a launch that grants databases still starts.
    if !options.sqlite.is_empty() {
        eprintln!(
            "livtet-plugin-host: --sqlite is accepted but the `sqlite` capability is\
             unavailable until stanchion#56 is resolved"
        );
    }

    // Load up front so the caller's first call is fast and a broken plugin root
    // surfaces before any request arrives.
    if let Some(root) = &config.plugins {
        fs_err::create_dir_all(root)
            .map_err(|e| format!("creating plugin root `{}`: {e}", root.display()))?;
        let report = registry
            .load_dir(root)
            .map_err(|err| format!("loading `{}`: {err}", root.display()))?;
        for failure in &report.failures {
            eprintln!("livtet-plugin-host: {failure}");
        }
    }

    serve(&mut registry, &channel, isolation).map_err(|err| format!("serving: {err}"))
}

const USAGE: &str = "\
livtet-plugin-host — run Livtet's Lua plugins in an isolated process

USAGE:
    livtet-plugin-host [--config FILE] [--plugins DIR] [--sqlite DB]...

OPTIONS:
    --config FILE   TOML host configuration (sandbox, capabilities, signatures)
    --plugins DIR   Plugin root to load at startup, overriding the config
    --sqlite DB     Accepted and ignored: the `sqlite` capability is unavailable
                    until stanchion#56 is resolved
    -h, --help      Print this message

The protocol is newline-delimited JSON on stdin/stdout (one object per line).
stderr is for logs.";

struct Options {
    config: Option<Utf8PathBuf>,
    plugins: Option<Utf8PathBuf>,
    sqlite: Vec<Utf8PathBuf>,
    help: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Options {
            config: None,
            plugins: None,
            sqlite: Vec::new(),
            help: false,
        };
        let mut args = args.peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => options.help = true,
                "--sqlite" => {
                    options.sqlite.push(Utf8PathBuf::from(
                        args.next().ok_or("--sqlite needs a path")?,
                    ));
                }
                "--config" => {
                    options.config = Some(Utf8PathBuf::from(
                        args.next().ok_or("--config needs a path")?,
                    ));
                }
                "--plugins" => {
                    options.plugins = Some(Utf8PathBuf::from(
                        args.next().ok_or("--plugins needs a path")?,
                    ));
                }
                other => return Err(format!("unexpected argument `{other}`\n\n{USAGE}")),
            }
        }
        Ok(options)
    }
}
