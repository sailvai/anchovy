//! Tauri commands for the Models screen. Thin wrappers over `Models`;
//! download progress reaches the interface as `model-progress` events and
//! a `models-changed` event when a download stops.

use super::{Event, Models, ModelsView};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

pub const PROGRESS_EVENT: &str = "model-progress";
pub const CHANGED_EVENT: &str = "models-changed";

#[derive(Clone, Serialize)]
struct ProgressPayload {
    id: String,
    downloaded: u64,
}

#[tauri::command]
pub fn list_models(models: State<Arc<Models>>) -> ModelsView {
    models.list()
}

#[tauri::command]
pub fn select_model(models: State<Arc<Models>>, id: String) -> Result<(), String> {
    models.select(&id).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn download_model(
    app: AppHandle,
    models: State<Arc<Models>>,
    id: String,
) -> Result<(), String> {
    models
        .download(&id, move |event| {
            let _ = match event {
                Event::Progress { id, downloaded } => {
                    app.emit(PROGRESS_EVENT, ProgressPayload { id, downloaded })
                }
                Event::Finished { id } => app.emit(CHANGED_EVENT, id),
            };
        })
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn cancel_model_download(models: State<Arc<Models>>, id: String) {
    models.cancel(&id);
}

#[tauri::command]
pub fn delete_model(models: State<Arc<Models>>, id: String) -> Result<(), String> {
    models.delete(&id).map_err(|err| err.to_string())
}
