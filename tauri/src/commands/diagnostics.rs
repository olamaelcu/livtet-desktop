//! In-app diagnostics: log export.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LogFile {
    pub filename: String,
    pub content: String,
}

#[tauri::command]
#[specta::specta]
#[tracing::instrument(skip(state), err)]
pub async fn export_logs(state: State<'_, AppState>) -> Result<Vec<LogFile>, String> {
    let logs_dir = state.logs_dir.clone();
    let entries: Vec<(String, String)> = tokio::task::spawn_blocking(move || {
        let mut files = Vec::new();
        let read_dir = std::fs::read_dir(&logs_dir).map_err(|e| e.to_string())?;
        for entry in read_dir {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !filename.ends_with(".log") {
                continue;
            }
            let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            files.push((filename.to_string(), content));
        }
        Ok::<_, String>(files)
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut files: Vec<LogFile> = entries
        .into_iter()
        .map(|(filename, content)| LogFile { filename, content })
        .collect();
    files.sort_by(|a, b| a.filename.cmp(&b.filename));
    Ok(files)
}
