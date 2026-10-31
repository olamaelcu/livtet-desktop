use crate::{atproto, error::AtprotoError, types::AppState};
use tauri::{Emitter, Manager, State};

const CALLBACK_SCHEME: &str = "net.olamaelcu.livtet:";

#[tauri::command]
#[specta::specta]
pub async fn atproto_begin_login(
    handle: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<atproto::AtprotoLoginStart, AtprotoError> {
    let manager = state.atproto.clone();
    let loopback = manager.loopback_redirect_uri().await?;
    let start = manager.begin_login(&handle, loopback.as_deref()).await?;

    // With a loopback redirect the browser comes back to us directly, so the
    // listener must be armed before the user approves anything.
    if let Some(redirect_uri) = loopback {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            match atproto::loopback::await_callback(&redirect_uri).await {
                Ok(callback_url) => match manager.complete_login(&callback_url).await {
                    Ok(status) => {
                        tracing::info!(did = %status.did, "ATProto sign-in completed");
                        let _ = app.emit("atproto://login", status);
                    }
                    Err(error) => {
                        tracing::warn!(%error, "ATProto sign-in failed");
                        let _ = app.emit("atproto://error", error.to_string());
                    }
                },
                Err(error) => {
                    tracing::warn!(%error, "ATProto loopback callback failed");
                    let _ = app.emit("atproto://error", error.to_string());
                }
            }
        });
    }
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
    match state.atproto.complete_login(&callback_url).await {
        Ok(status) => Ok(status),
        Err(error) => {
            // The manual path has no deep-link handler to log for it.
            tracing::warn!(%error, "ATProto manual completion failed");
            Err(error)
        }
    }
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

/// Give the stored session a fresh access token, using the refresh token.
///
/// Settings offers this when the expiry it shows has already passed. A token that
/// is still comfortably valid is left alone, so pressing it never spends a
/// refresh token for nothing; `None` means nobody is signed in.
#[tauri::command]
#[specta::specta]
pub async fn atproto_refresh_session(
    state: State<'_, AppState>,
) -> Result<Option<atproto::AtprotoStatus>, AtprotoError> {
    state.atproto.refresh_session().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_get_profile(
    state: State<'_, AppState>,
) -> Result<Option<atproto::AtprotoProfileView>, AtprotoError> {
    state.atproto.get_profile().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_update_profile(
    display_name: String,
    description: String,
    state: State<'_, AppState>,
) -> Result<atproto::AtprotoProfileView, AtprotoError> {
    state
        .atproto
        .update_profile(display_name, description)
        .await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_set_avatar(
    avatar_path: String,
    state: State<'_, AppState>,
) -> Result<atproto::AtprotoProfileView, AtprotoError> {
    state.atproto.set_avatar(&avatar_path).await
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
        tracing::info!(url = %url, "ATProto callback received");
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let manager = app.state::<AppState>().atproto.clone();
            match manager.complete_login(&url).await {
                Ok(status) => {
                    tracing::info!(did = %status.did, "ATProto sign-in completed");
                    let _ = app.emit("atproto://login", status);
                }
                Err(error) => {
                    tracing::warn!(%error, "ATProto sign-in failed");
                    let _ = app.emit("atproto://error", error.to_string());
                }
            }
        });
    }
}
