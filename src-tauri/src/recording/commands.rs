//! Tauri commands for recording. Thin wrappers over `core::Recorder` and
//! `mac`; while recording, elapsed time and file size reach the interface as
//! a `recording-progress` event once a second.

use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use super::core::{ComputerAudio, Recorder, Recording, Saved, PROGRESS_INTERVAL};
use super::mac;
use crate::library::Library;
use crate::setup::can_record;

pub const PROGRESS_EVENT: &str = "recording-progress";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputDeviceView {
    pub uid: String,
    pub name: String,
    pub is_default: bool,
}

/// The two lines of the sources area before recording.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sources {
    /// System name of the microphone that Record will use.
    pub microphone: Option<String>,
    /// How the last recording in this run went; `None` before the first.
    pub computer_audio: Option<ComputerAudio>,
}

#[tauri::command]
pub fn list_input_devices() -> Result<Vec<InputDeviceView>, String> {
    let default = mac::default_input_uid();
    let devices = mac::input_devices().map_err(|err| err.to_string())?;
    Ok(devices
        .into_iter()
        .map(|d| InputDeviceView {
            is_default: default.as_deref() == Some(&d.uid[..]),
            uid: d.uid,
            name: d.name,
        })
        .collect())
}

#[tauri::command]
pub fn recording_sources(
    recorder: State<Arc<Recorder>>,
    device_uid: Option<String>,
) -> Result<Sources, String> {
    let devices = mac::input_devices().map_err(|err| err.to_string())?;
    let default = mac::default_input_uid();
    let microphone =
        super::core::pick_microphone(&devices, device_uid.as_deref(), default.as_deref())
            .map(|d| d.name.clone());
    Ok(Sources {
        microphone,
        computer_audio: recorder.last_computer_audio(),
    })
}

#[tauri::command]
pub fn start_recording(
    app: AppHandle,
    recorder: State<Arc<Recorder>>,
    library: State<Library>,
    device_uid: Option<String>,
) -> Result<Recording, String> {
    // The same folder the library lists, so a new recording shows up there.
    let notes_dir = library.notes_dir();
    let microphone = crate::setup::mac::microphone_access();
    if !can_record(notes_dir.is_some(), microphone) {
        return Err(match notes_dir {
            None => "Choose a notes folder before recording.".into(),
            Some(_) => "Anchovy needs microphone access to record.".into(),
        });
    }
    let notes_dir = notes_dir.unwrap_or_default();
    recorder
        .start(
            &notes_dir,
            mac::local_now(),
            || mac::start(device_uid.as_deref()),
            PROGRESS_INTERVAL,
            move |progress| {
                let _ = app.emit(PROGRESS_EVENT, progress);
            },
        )
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn stop_recording(recorder: State<Arc<Recorder>>) -> Result<Saved, String> {
    recorder.stop().map_err(|err| err.to_string())
}
