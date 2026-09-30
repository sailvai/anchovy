//! Tauri commands for notes. Thin wrappers over `Pipeline`; the pipeline's
//! events reach the interface as `note-progress` and `notes-changed`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{Event, Pipeline, Stage};
use crate::library::Library;

pub const PROGRESS_EVENT: &str = "note-progress";
pub const CHANGED_EVENT: &str = "notes-changed";

#[derive(Clone, Serialize)]
struct ProgressPayload {
    folder: String,
    #[serde(flatten)]
    stage: Stage,
}

fn folder_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Sends pipeline events to the interface, by folder name.
pub fn emitter(app: AppHandle) -> impl Fn(Event) + Send + Sync + 'static {
    move |event| {
        let _ = match event {
            Event::Changed { folder } => app.emit(CHANGED_EVENT, folder_name(&folder)),
            Event::Progress { folder, stage } => app.emit(
                PROGRESS_EVENT,
                ProgressPayload {
                    folder: folder_name(&folder),
                    stage,
                },
            ),
        };
    }
}

/// Every recording folder in the notes folder.
pub fn recording_folders(library: &Library) -> Vec<PathBuf> {
    library
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|recording| library.path_of(&recording.folder).ok())
        .collect()
}

/// Generate note, Retry, and Regenerate note. `replace` is the answer to
/// "Replace note.md?".
#[tauri::command]
pub fn generate_note(
    pipeline: State<Arc<Pipeline>>,
    library: State<Library>,
    folder: String,
    replace: bool,
) -> Result<(), String> {
    let path = library.path_of(&folder).map_err(|err| err.to_string())?;
    pipeline
        .generate(&path, replace)
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Where each note being written is, by folder name.
#[tauri::command]
pub fn note_progress(
    pipeline: State<Arc<Pipeline>>,
    library: State<Library>,
) -> HashMap<String, Stage> {
    recording_folders(&library)
        .into_iter()
        .filter_map(|path| Some((folder_name(&path), pipeline.stage(&path)?)))
        .collect()
}

/// After a download: start the notes that were waiting for models.
#[tauri::command]
pub fn resume_waiting_notes(pipeline: State<Arc<Pipeline>>, library: State<Library>) {
    pipeline.resume_waiting(&recording_folders(&library));
}
