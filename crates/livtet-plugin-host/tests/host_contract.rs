use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use stanchion::remote::{RemoteOptions, RemoteRegistry};

const INCOMPLETE_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer

function Importer.new(config, deps)
  return setmetatable({ config = config, deps = deps }, Importer)
end

return Importer
"#;

const PARTIAL_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer

function Importer.new(config, deps)
  return setmetatable({ config = config, deps = deps }, Importer)
end

function Importer:extensions()
  return { "txt" }
end

return Importer
"#;

fn host_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_livtet-plugin-host"))
}

fn write_partial_plugin(root: &Path) {
    let dir = root.join("partial-importer");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.toml"),
        "name = \"partial-importer\"\nversion = \"1.0.0\"\n\n[capabilities.fs_read]\n",
    )
    .unwrap();
    fs::write(dir.join("init.lua"), PARTIAL_IMPORTER).unwrap();
}

fn write_plugin(root: &Path, name: &str, source: &str) {
    let dir = root.join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.toml"),
        format!("name = \"{name}\"\nversion = \"1.0.0\"\n\n[capabilities.fs_read]\n"),
    )
    .unwrap();
    fs::write(dir.join("init.lua"), source).unwrap();
    fs::write(
        root.join("host.toml"),
        "[capabilities]\ncallbacks = [\"fs_read\"]\n\n[signatures]\nrequired = false\n",
    )
    .unwrap();
}

#[test]
fn incomplete_importer_fails_host_startup() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "not-an-importer", INCOMPLETE_IMPORTER);
    write_partial_plugin(root.path());

    let output = Command::new(host_binary())
        .arg("--config")
        .arg(root.path().join("host.toml"))
        .arg("--plugins")
        .arg(root.path())
        .output()
        .expect("plugin host starts");

    assert!(
        !output.status.success(),
        "an incomplete importer must fail fast"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not-an-importer") && stderr.contains("read_metadata"),
        "unexpected host stderr: {stderr}"
    );
    assert!(
        stderr.contains("partial-importer"),
        "an importer with extensions but no read_metadata must also fail: {stderr}"
    );
}

#[test]
fn contract_report_mode_lists_without_serving_violations() {
    let root = tempfile::tempdir().unwrap();
    write_plugin(root.path(), "partial-importer", PARTIAL_IMPORTER);
    let options = RemoteOptions::new(host_binary())
        .config(root.path().join("host.toml"))
        .plugins(root.path())
        .inherit_stderr(false)
        .arg("--contract")
        .arg("report");
    let mut remote = RemoteRegistry::launch(options).expect("report-mode host starts");

    let plugins = remote.list().expect("report mode preserves introspection");
    let plugin = plugins
        .iter()
        .find(|plugin| plugin.name == "partial-importer")
        .expect("partial importer is listed");
    assert!(
        plugin.granted.is_empty(),
        "report mode must strip capabilities from contract violations"
    );

    let error = remote
        .call::<String>("partial-importer", "read_metadata", [])
        .expect_err("missing contract methods must not serve");
    assert!(
        error.to_string().contains("read_metadata"),
        "unexpected call error: {error}"
    );
    assert!(remote.is_alive());
    remote.shutdown().expect("report-mode host shuts down");
}
