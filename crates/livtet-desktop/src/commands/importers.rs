use std::fs::File;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::{Engine, engine::general_purpose::STANDARD};
use livtet_importer::ImporterMeta;
use serde_json::{Value as Json, json};
use stanchion::remote::{CallbackCall, RemoteOptions, RemoteRegistry};

use super::import::ImportError;

const MAX_FS_READ_BYTES: u64 = 33_554_432;
const MAX_REMOTE_TEXT_CHARS: usize = 32_768;
const MAX_REMOTE_DESCRIPTION_CHARS: usize = 1_000_000;
const MAX_REMOTE_LIST_ITEMS: usize = 10_000;
const MAX_REMOTE_COVER_BASE64_BYTES: u64 = 16_777_216;

fn check_byte_len(label: &str, len: u64, max: u64) -> Result<(), String> {
    if len > max {
        return Err(format!("{label} exceeds the {max}-byte remote limit"));
    }
    Ok(())
}

fn check_remote_text(label: &str, value: &str) -> Result<(), ImportError> {
    if value.chars().count() > MAX_REMOTE_TEXT_CHARS {
        return Err(ImportError::new(
            "importer",
            format!("{label} exceeds the remote text limit"),
        ));
    }
    Ok(())
}

fn check_remote_count(label: &str, len: usize) -> Result<(), ImportError> {
    if len > MAX_REMOTE_LIST_ITEMS {
        return Err(ImportError::new(
            "importer",
            format!("{label} exceeds the remote list limit"),
        ));
    }
    Ok(())
}

fn check_remote_payload(record: &ImporterMeta) -> Result<(), ImportError> {
    check_remote_text("title", &record.title)?;
    check_remote_count("contributors", record.contributors.len())?;
    for contributor in &record.contributors {
        check_remote_text("contributor.name", &contributor.name)?;
        check_remote_text(
            "contributor.role",
            contributor.role.as_deref().unwrap_or_default(),
        )?;
        check_remote_text(
            "contributor.file_as",
            contributor.file_as.as_deref().unwrap_or_default(),
        )?;
    }
    check_remote_count("isbns", record.isbns.len())?;
    for isbn in &record.isbns {
        check_remote_text("isbn", isbn)?;
    }
    check_remote_count("other_identifiers", record.other_identifiers.len())?;
    for identifier in &record.other_identifiers {
        check_remote_text("other_identifier", identifier)?;
    }
    check_remote_text("publisher", record.publisher.as_deref().unwrap_or_default())?;
    check_remote_text("language", record.language.as_deref().unwrap_or_default())?;
    if record
        .description
        .as_deref()
        .unwrap_or_default()
        .chars()
        .count()
        > MAX_REMOTE_DESCRIPTION_CHARS
    {
        return Err(ImportError::new(
            "importer",
            "description exceeds the remote text limit",
        ));
    }
    check_remote_count("subjects", record.subjects.len())?;
    for subject in &record.subjects {
        check_remote_text("subject", subject)?;
    }
    if let Some(cover) = &record.cover {
        check_remote_text("cover.mime", &cover.mime)?;
        let encoded_len = u64::try_from(cover.data_base64.len()).map_err(|_| {
            ImportError::new("importer", "cover size does not fit in a file length")
        })?;
        check_byte_len("cover", encoded_len, MAX_REMOTE_COVER_BASE64_BYTES)
            .map_err(|error| ImportError::new("importer", error))?;
    }
    Ok(())
}

/// Read one file through a remote Lua importer.
///
/// The host is launched per call and answers only `fs_read` for the selected
/// file. The registry stays on its blocking worker because it is not `Send`.
/// Metadata from a remote importer, paired with the selected-file bytes the
/// host actually served through `fs_read`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteImporterRead {
    pub meta: ImporterMeta,
    pub source_bytes: Vec<u8>,
}

pub async fn remote_importer_metadata(
    host: PathBuf,
    config: PathBuf,
    plugins_dir: PathBuf,
    path: PathBuf,
) -> Result<RemoteImporterRead, ImportError> {
    let selected_path = path.clone();
    let observed_bytes = Arc::new(Mutex::new(None));
    let record = tokio::task::spawn_blocking(move || -> Result<RemoteImporterRead, ImportError> {
        let requested_path = selected_path.to_string_lossy().into_owned();
        let extension = selected_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let options = RemoteOptions::new(host)
            .config(config)
            .plugins(&plugins_dir)
            .inherit_stderr(false);
        let remote =
            RemoteRegistry::launch(options).map_err(|error| ImportError::new("importer", error))?;
        let observed_for_callback = Arc::clone(&observed_bytes);
        let callback_path = selected_path.clone();
        let mut remote = remote.on_callback(move |call| {
            let response = answer_fs_read(call, &callback_path)?;
            if let Some(encoded) = response.get("data_base64").and_then(Json::as_str)
                && let Ok(bytes) = STANDARD.decode(encoded)
                && let Ok(mut observed) = observed_for_callback.lock()
            {
                observed.get_or_insert(bytes);
            }
            Ok(response)
        });

        let mut plugin_names = Vec::new();
        let outcomes = remote
            .dispatch("extensions", [])
            .map_err(|error| ImportError::new("importer", error))?;
        for outcome in outcomes {
            let Some(value) = outcome.value else { continue };
            let extensions: Vec<String> = serde_json::from_value(value)
                .map_err(|error| ImportError::new("importer", error))?;
            if extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(&extension))
            {
                plugin_names.push(outcome.plugin);
            }
        }
        let plugin_name = match plugin_names.as_slice() {
            [plugin_name] => plugin_name.clone(),
            [] => {
                return Err(ImportError::new(
                    "unsupported",
                    no_importer_message(&extension),
                ));
            }
            _ => {
                plugin_names.sort();
                return Err(ImportError::new(
                    "importer",
                    format!(
                        "multiple importers claim .{extension}: {}",
                        plugin_names.join(", ")
                    ),
                ));
            }
        };
        let raw: Json = remote
            .call(&plugin_name, "read_metadata", [json!(requested_path)])
            .map_err(|error| ImportError::new("importer", error))?;
        let record = ImporterMeta::from_wire_json(raw)
            .map_err(|error| ImportError::new("importer", error))?;
        check_remote_payload(&record)?;
        let source_bytes = match observed_bytes
            .lock()
            .map_err(|error| ImportError::new("importer", error))?
            .clone()
        {
            Some(bytes) => bytes,
            None => std::fs::read(&selected_path)
                .map_err(|error| ImportError::new("io", format!("reading import file: {error}")))?,
        };
        Ok(RemoteImporterRead {
            meta: record,
            source_bytes,
        })
    })
    .await
    .map_err(|error| ImportError::new("importer", error))??;
    Ok(record)
}

fn no_importer_message(extension: &str) -> String {
    if extension.is_empty() {
        "no importer for files without an extension".to_string()
    } else {
        format!("no importer for .{extension}")
    }
}

fn answer_fs_read(call: &CallbackCall, selected_path: &Path) -> Result<Json, String> {
    if call.capability != "fs_read" {
        return Err(format!("unsupported capability `{}`", call.capability));
    }
    let [requested] = call.args.as_slice() else {
        return Err("fs_read needs exactly one path".to_string());
    };
    let requested = requested
        .as_str()
        .ok_or_else(|| "fs_read path must be a string".to_string())?;
    if Path::new(requested) != selected_path {
        return Err("fs_read is limited to the selected import file".to_string());
    }
    let file =
        File::open(selected_path).map_err(|error| format!("reading import file: {error}"))?;
    let advertised = file
        .metadata()
        .map_err(|error| format!("reading import file: {error}"))?
        .len();
    check_byte_len("selected file", advertised, MAX_FS_READ_BYTES)?;
    let mut limited = file.take(MAX_FS_READ_BYTES + 1);
    let mut bytes = Vec::new();
    limited
        .read_to_end(&mut bytes)
        .map_err(|error| format!("reading import file: {error}"))?;
    let read = u64::try_from(bytes.len())
        .map_err(|_| "selected file size does not fit in a file length".to_string())?;
    check_byte_len("selected file", read, MAX_FS_READ_BYTES)?;
    Ok(json!({ "data_base64": STANDARD.encode(bytes) }))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use base64::{Engine, engine::general_purpose::STANDARD};

    use stanchion::remote::CallbackCall;

    use super::{
        MAX_FS_READ_BYTES, MAX_REMOTE_COVER_BASE64_BYTES, answer_fs_read, check_byte_len,
        check_remote_payload, no_importer_message, remote_importer_metadata,
    };
    use crate::commands::import::ImportError;

    const TEXT_IMPORTER: &str = r#"
local Importer = {}
Importer.__index = Importer

function Importer.new(config, deps)
  return setmetatable({}, Importer)
end

function Importer:extensions()
  return { "TXT" }
end

function Importer:read_metadata(path)
  local response = fs_read(path)
  return {
    title = "Remote Title",
    contributors = {{ name = "Remote Author", role = "aut" }},
    isbns = { "9781784780609" },
    other_identifiers = {},
    language = "en",
    description = "Remote description.",
    subjects = { "Remote" },
    cover = { mime = "text/plain", data_base64 = response.data_base64 },
  }
end

return Importer
"#;

    fn host_binary() -> PathBuf {
        let test_binary = std::env::current_exe().unwrap();
        let profile_dir = test_binary.parent().and_then(|path| path.parent()).unwrap();
        profile_dir.join(if cfg!(windows) {
            "livtet-plugin-host.exe"
        } else {
            "livtet-plugin-host"
        })
    }

    fn write_txt_plugin(plugins_dir: &std::path::Path, name: &str) {
        let plugin_dir = plugins_dir.join(name);
        fs::create_dir_all(&plugin_dir).unwrap();
        fs::write(
            plugin_dir.join("plugin.toml"),
            format!(
                "name = \"{name}\"\nversion = \"1.0.0\"\n\n[capabilities.log]\n\n[capabilities.fs_read]\n"
            ),
        )
        .unwrap();
        fs::write(plugin_dir.join("init.lua"), TEXT_IMPORTER).unwrap();
    }

    fn write_host_config(root: &tempfile::TempDir) -> PathBuf {
        let config = root.path().join("host.toml");
        fs::write(
            &config,
            "[capabilities]\nallow = [\"log\"]\ncallbacks = [\"fs_read\"]\n\n[signatures]\nrequired = false\n",
        )
        .unwrap();
        config
    }

    #[tokio::test]
    async fn remote_importer_reads_only_the_selected_file() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("remote.txt");
        fs::write(&file, b"remote-bytes").unwrap();

        let plugins_dir = root.path().join("plugins");
        write_txt_plugin(&plugins_dir, "txt-importer");
        let config = write_host_config(&root);

        let read = remote_importer_metadata(host_binary(), config, plugins_dir, file.clone())
            .await
            .expect("remote importer reads the selected file");
        let record = read.meta;

        assert_eq!(read.source_bytes, b"remote-bytes".to_vec());
        assert_eq!(record.title, "Remote Title");
        assert_eq!(record.isbns, vec!["9781784780609".to_string()]);
        let cover = record.cover.expect("remote cover is preserved");
        assert_eq!(STANDARD.decode(cover.data_base64).unwrap(), b"remote-bytes");
    }

    #[tokio::test]
    async fn duplicate_extension_claims_are_an_error() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("remote.txt");
        fs::write(&file, b"remote-bytes").unwrap();

        let plugins_dir = root.path().join("plugins");
        write_txt_plugin(&plugins_dir, "txt-importer");
        write_txt_plugin(&plugins_dir, "other-txt-importer");
        let config = write_host_config(&root);

        let ImportError { code, message } =
            remote_importer_metadata(host_binary(), config, plugins_dir, file)
                .await
                .expect_err("ambiguous extension claims must fail");
        assert_eq!(code, "importer");
        assert!(
            message.contains("multiple importers claim .txt"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn extensionless_files_name_the_problem() {
        assert_eq!(
            no_importer_message(""),
            "no importer for files without an extension"
        );
        assert_eq!(no_importer_message("txt"), "no importer for .txt");
    }

    #[test]
    fn fs_read_rejects_a_different_path() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("selected.txt");
        fs::write(&selected, b"selected").unwrap();
        let other = root.path().join("other.txt");
        fs::write(&other, b"other").unwrap();
        let call = CallbackCall {
            plugin: "txt-importer".to_string(),
            capability: "fs_read".to_string(),
            grant: serde_json::Value::Null,
            args: vec![serde_json::json!(other.to_string_lossy())],
        };

        let error = answer_fs_read(&call, &selected).expect_err("fs_read is scoped");
        assert_eq!(error, "fs_read is limited to the selected import file");
    }

    #[test]
    fn byte_limits_reject_payloads_above_operational_bounds() {
        assert!(check_byte_len("selected file", MAX_FS_READ_BYTES, MAX_FS_READ_BYTES).is_ok());
        assert!(check_byte_len("selected file", MAX_FS_READ_BYTES + 1, MAX_FS_READ_BYTES).is_err());
        assert!(
            check_byte_len(
                "cover",
                MAX_REMOTE_COVER_BASE64_BYTES,
                MAX_REMOTE_COVER_BASE64_BYTES
            )
            .is_ok()
        );
        assert!(
            check_byte_len(
                "cover",
                MAX_REMOTE_COVER_BASE64_BYTES + 1,
                MAX_REMOTE_COVER_BASE64_BYTES
            )
            .is_err()
        );
    }

    #[test]
    fn remote_payload_rejects_an_unbounded_title() {
        let record = livtet_importer::ImporterMeta {
            title: "x".repeat(32_769),
            contributors: Vec::new(),
            isbns: Vec::new(),
            other_identifiers: Vec::new(),
            publisher: None,
            language: None,
            published: None,
            description: None,
            subjects: Vec::new(),
            cover: None,
        };

        let ImportError { code, message } =
            check_remote_payload(&record).expect_err("remote text must be bounded");
        assert_eq!(code, "importer");
        assert!(
            message.contains("title"),
            "unexpected payload error: {message}"
        );
    }

    #[test]
    fn fs_read_rejects_an_undeclared_capability() {
        let root = tempfile::tempdir().unwrap();
        let selected = root.path().join("selected.txt");
        fs::write(&selected, b"selected").unwrap();
        let call = CallbackCall {
            plugin: "txt-importer".to_string(),
            capability: "network".to_string(),
            grant: serde_json::Value::Null,
            args: vec![serde_json::json!(selected.to_string_lossy())],
        };

        let error = answer_fs_read(&call, &selected).expect_err("only fs_read is answered");
        assert_eq!(error, "unsupported capability `network`");
    }
}
