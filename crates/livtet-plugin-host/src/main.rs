//! Livtet importer plugin host.
//!
//! The host runs Lua importers in an isolated child process and speaks
//! length-prefixed JSON over stdio. Unlike the generic Stanchion host, it
//! refuses to serve when any loaded plugin does not implement the importer
//! contract.

use std::io::{self, BufReader, BufWriter};
use std::path::PathBuf;
use std::process::ExitCode;

use livtet_importer::ImporterHandle;
use stanchion::{
    LuaObject,
    registry::{DynClass, Registry},
    remote::{HostChannel, build_registry, load_config, serve},
};

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
        Some(path) => load_config(path)?,
        None => Default::default(),
    };
    if let Some(plugins) = options.plugins {
        config.plugins = Some(plugins);
    }

    let channel = HostChannel::new(BufReader::new(io::stdin()), BufWriter::new(io::stdout()));
    let mut registry = build_registry(&config, &channel)?;

    if let Some(root) = &config.plugins {
        let report = registry
            .load_dir(root)
            .map_err(|err| format!("loading `{}`: {err}", root.display()))?;
        for failure in &report.failures {
            eprintln!("livtet-plugin-host: {failure}");
        }

        let violations = importer_violations(&registry);
        if !violations.is_empty() {
            for violation in &violations {
                eprintln!("livtet-plugin-host: {violation}");
            }
            if options.contract == ContractMode::Reject {
                return Err(format!(
                    "refusing to serve {} plugin(s) without the importer contract: {}",
                    violations.len(),
                    violations.join("; ")
                ));
            }
            let revocations: Vec<(String, Vec<String>)> = registry
                .plugins()
                .iter()
                .map(|plugin| {
                    (
                        plugin.name().to_string(),
                        plugin.granted_capabilities().map(str::to_string).collect(),
                    )
                })
                .collect();
            for (plugin, capabilities) in revocations {
                for capability in capabilities {
                    registry.revoke(&plugin, &capability).map_err(|error| {
                        format!("revoking {capability} from `{plugin}`: {error}")
                    })?;
                }
            }
        }
    }

    serve(&mut registry, &channel).map_err(|err| format!("serving: {err}"))
}

fn importer_violations(registry: &Registry<DynClass>) -> Vec<String> {
    let mut violations = Vec::new();

    for plugin in registry.plugins() {
        for method in ImporterHandle::required_methods() {
            match plugin.instance().has_method(method) {
                Ok(true) => {}
                Ok(false) => violations.push(format!(
                    "plugin `{}` is missing required importer method `{method}`",
                    plugin.name()
                )),
                Err(error) => violations.push(format!(
                    "plugin `{}` failed the importer contract check for `{method}`: {error}",
                    plugin.name()
                )),
            }
        }
    }

    violations
}

const USAGE: &str = "\
livtet-plugin-host — run Livtet importer plugins in an isolated process

USAGE:
    livtet-plugin-host [--config FILE] [--plugins DIR] [--contract reject|report]

OPTIONS:
    --config FILE   TOML host configuration (sandbox, capabilities, signatures)
    --plugins DIR   Plugin root to load at startup, overriding the config
    --contract MODE Contract violations fail startup (reject, the default) or strip
                    capabilities and serve introspection only (report)
    -h, --help      Print this message

The protocol is length-prefixed JSON on stdin/stdout. stderr is for logs.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ContractMode {
    #[default]
    Reject,
    Report,
}

struct Options {
    config: Option<PathBuf>,
    plugins: Option<PathBuf>,
    contract: ContractMode,
    help: bool,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut options = Options {
            config: None,
            plugins: None,
            contract: ContractMode::Reject,
            help: false,
        };
        let mut args = args.peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-h" | "--help" => options.help = true,
                "--config" => {
                    options.config =
                        Some(PathBuf::from(args.next().ok_or("--config needs a path")?));
                }
                "--plugins" => {
                    options.plugins =
                        Some(PathBuf::from(args.next().ok_or("--plugins needs a path")?));
                }
                "--contract" => {
                    let mode = args.next().ok_or("--contract needs reject or report")?;
                    options.contract = match mode.as_str() {
                        "reject" => ContractMode::Reject,
                        "report" => ContractMode::Report,
                        other => {
                            return Err(format!("unexpected contract mode `{other}`\n\n{USAGE}"));
                        }
                    };
                }
                other => return Err(format!("unexpected argument `{other}`\n\n{USAGE}")),
            }
        }
        Ok(options)
    }
}
