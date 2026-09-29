//! Tauri commands for the recording library. Thin wrappers over `Library`.

use super::{mac, Library, NoteView, Recording};
use tauri::{AppHandle, State};

#[tauri::command]
pub fn list_recordings(library: State<Library>) -> Result<Vec<Recording>, String> {
    library.list().map_err(|err| err.to_string())
}

#[tauri::command]
pub fn read_note(library: State<Library>, folder: String) -> Result<NoteView, String> {
    library.read_note(&folder).map_err(|err| err.to_string())
}

#[tauri::command]
pub fn show_in_finder(
    app: AppHandle,
    library: State<Library>,
    folder: String,
) -> Result<(), String> {
    let path = library.path_of(&folder).map_err(|err| err.to_string())?;
    // AppKit wants the main thread.
    app.run_on_main_thread(move || {
        let _ = mac::reveal(&path);
    })
    .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn move_to_trash(library: State<Library>, folder: String) -> Result<(), String> {
    library
        .move_to_trash(&folder, |path| mac::trash(path).map(|_| ()))
        .map_err(|err| err.to_string())
}
