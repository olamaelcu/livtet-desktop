use crate::{atproto, error::AtprotoError};
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn atproto_begin_login(
    handle: String,
    state: State<'_, crate::types::AppState>,
) -> Result<atproto::AtprotoLoginStart, AtprotoError> {
    state.atproto.begin_login(&handle).await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_complete_login(
    callback_url: String,
    state: State<'_, crate::types::AppState>,
) -> Result<atproto::AtprotoStatus, AtprotoError> {
    state.atproto.complete_login(&callback_url).await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_status(
    state: State<'_, crate::types::AppState>,
) -> Result<Option<atproto::AtprotoStatus>, AtprotoError> {
    state.atproto.status().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_logout(
    state: State<'_, crate::types::AppState>,
) -> Result<(), AtprotoError> {
    state.atproto.logout().await
}

#[tauri::command]
#[specta::specta]
pub async fn atproto_get_profile(
    state: State<'_, crate::types::AppState>,
) -> Result<Option<atproto::AtprotoProfileView>, AtprotoError> {
    state.atproto.get_profile().await
}
