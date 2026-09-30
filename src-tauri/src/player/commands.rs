//! Tauri commands for the note's player. Thin wrappers over `player`.

use tauri::{AppHandle, Manager, State};

use super::RecordingAudio;
use crate::library::Library;

/// The recording's `audio.wav` or `audio.m4a` for the player, or `None`
/// before it has audio. The interface passes only this path to
/// `convertFileSrc`.
#[tauri::command]
pub fn recording_audio(
    app: AppHandle,
    library: State<Library>,
    folder: String,
) -> Result<Option<RecordingAudio>, String> {
    super::recording_audio(&library, &app.asset_protocol_scope(), &folder)
}
