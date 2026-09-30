//! Tauri commands for the Settings screen. Thin wrappers over `SettingsStore`.
//! The notes folder is changed with `choose_notes_folder` in `folder_access`.

use std::sync::Arc;

use tauri::State;

use super::{Settings, SettingsStore};

#[tauri::command]
pub fn get_settings(store: State<Arc<SettingsStore>>) -> Settings {
    store.get()
}

/// Saves the settings and returns them as saved. A recording in progress
/// keeps the device and quality it started with.
#[tauri::command]
pub fn update_settings(
    store: State<Arc<SettingsStore>>,
    settings: Settings,
) -> Result<Settings, String> {
    store
        .update(settings)
        .map_err(|err| format!("Anchovy couldn't save the settings. {err}"))
}
