use crate::{atproto, error::AtprotoError, types::AppState};
use tauri::{Emitter, Manager, State};

const CALLBACK_SCHEME: &str = "net.olamaelcu.livtet:";

#[tauri::command]
#[specta::specta]
pub async fn atproto_begin_login(
    handle: String,
    state: State<'_, AppState>,
) -> Result<atproto::AtprotoLoginStart, AtprotoError> {
    let start = state.atproto.begin_login(&handle).await?;
    // The browser is the only place the authorization UI lives; a failed
    // hand-off is not fatal because the URL is returned to the caller too.
    if let Err(error) = tauri_plugin_opener::open_url(&start.url, None::<&str>) {
        tracing::warn!(%error, "could not open the ATProto authorization URL");
    }
    Ok(start)
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_complete_login(
    callback_url: String,
    state: State<'_, AppState>,
) -> Result<atproto::AtprotoStatus, AtprotoError> {
    state.atproto.complete_login(&callback_url).await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_status(
    state: State<'_, AppState>,
) -> Result<Option<atproto::AtprotoStatus>, AtprotoError> {
    state.atproto.status().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_logout(state: State<'_, AppState>) -> Result<(), AtprotoError> {
    state.atproto.logout().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_get_profile(
    state: State<'_, AppState>,
) -> Result<Option<atproto::AtprotoProfileView>, AtprotoError> {
    state.atproto.get_profile().await
}

/// Complete any OAuth callback the OS handed to the app.
///
/// Each URL runs on its own task so a slow token exchange never blocks the
/// deep-link event loop, and the result reaches the UI as an event because no
/// command is waiting on it.
pub(crate) fn handle_deep_links(app: &tauri::AppHandle, urls: Vec<url::Url>) {
    for url in urls {
        let url = url.to_string();
        if !url.starts_with(CALLBACK_SCHEME) {
            continue;
        }
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let manager = app.state::<AppState>().atproto.clone();
            match manager.complete_login(&url).await {
                Ok(status) => {
                    let _ = app.emit("atproto://login", status);
                }
                Err(error) => {
                    let _ = app.emit("atproto://error", error.to_string());
                }
            }
        });
    }
}
