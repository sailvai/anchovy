//! Tauri commands for the notes folder. Thin wrappers over `FolderAccess`
//! and the folder panels in `mac`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use objc2::MainThreadMarker;
use tauri::{AppHandle, State};

use super::mac::{self, MacBookmarks};
use super::{default_folder, describe, FolderAccess, NotesFolder};
use crate::library::Library;
use crate::recording::core::Recorder;

pub type NotesAccess = FolderAccess<MacBookmarks>;

/// Runs `f` on the main thread, where AppKit panels must run, and waits for
/// it without blocking the command's thread.
async fn on_main_thread<T: Send + 'static>(
    app: &AppHandle,
    f: impl FnOnce(MainThreadMarker) -> T + Send + 'static,
) -> Result<T, String> {
    let (tx, mut rx) = tauri::async_runtime::channel(1);
    app.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("run_on_main_thread runs on the main thread");
        let _ = tx.try_send(f(mtm));
    })
    .map_err(|err| err.to_string())?;
    rx.recv()
        .await
        .ok_or_else(|| "The folder panel closed unexpectedly.".to_string())
}

fn home() -> Result<PathBuf, String> {
    mac::user_home().map_err(|err| err.to_string())
}

fn use_folder(
    access: &NotesAccess,
    library: &Library,
    folder: &Path,
    home: &Path,
) -> Result<NotesFolder, String> {
    let folder = access.choose(folder).map_err(|err| err.to_string())?;
    library.set_notes_dir(folder.clone());
    Ok(describe(&folder, home))
}

/// "Choose Folder…" on first launch and Change… in Settings: any folder,
/// including an existing Obsidian vault. `None` if the user cancels. Not
/// while recording: the recording is written into the notes folder. A note
/// being written finishes in the folder it started in.
#[tauri::command]
pub async fn choose_notes_folder(
    app: AppHandle,
    access: State<'_, NotesAccess>,
    library: State<'_, Library>,
    recorder: State<'_, Arc<Recorder>>,
) -> Result<Option<NotesFolder>, String> {
    if recorder.is_recording() {
        return Err("Stop recording before changing the notes folder.".into());
    }
    let home = home()?;
    let start = library
        .notes_dir()
        .unwrap_or_else(|| home.join("Documents"));
    let Some(folder) = on_main_thread(&app, move |mtm| mac::choose_folder(mtm, &start)).await?
    else {
        return Ok(None);
    };
    use_folder(&access, &library, &folder, &home).map(Some)
}

/// Continue with `~/Documents/Anchovy`. Inside the sandbox the user confirms
/// it in a panel first, which is what lets Anchovy create and keep it.
/// `None` if the user cancels.
#[tauri::command]
pub async fn use_default_notes_folder(
    app: AppHandle,
    access: State<'_, NotesAccess>,
    library: State<'_, Library>,
) -> Result<Option<NotesFolder>, String> {
    let home = home()?;
    let folder = default_folder(&home);
    if let Ok(chosen) = use_folder(&access, &library, &folder, &home) {
        return Ok(Some(chosen));
    }
    let Some(confirmed) =
        on_main_thread(&app, move |mtm| mac::confirm_folder(mtm, &folder)).await?
    else {
        return Ok(None);
    };
    use_folder(&access, &library, &confirmed, &home).map(Some)
}
