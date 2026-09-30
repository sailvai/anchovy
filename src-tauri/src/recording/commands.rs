//! Tauri commands for recording. Thin wrappers over `core::Recorder` and
//! `mac`; a start reaches the interface as a `recording-started` event, and
//! while recording, elapsed time and file size as a `recording-progress`
//! event once a second.

use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use super::core::{ComputerAudio, Recorder, Recording, Saved, PROGRESS_INTERVAL};
use super::mac;
use crate::library::Library;
use crate::notes::note::Source;
use crate::pipeline::Pipeline;
use crate::settings::SettingsStore;
use crate::setup::can_record;

pub const PROGRESS_EVENT: &str = "recording-progress";
pub const STARTED_EVENT: &str = "recording-started";

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

/// The sources Record will use: the saved input device, or the system
/// default when it is not connected.
#[tauri::command]
pub fn recording_sources(
    recorder: State<Arc<Recorder>>,
    settings: State<Arc<SettingsStore>>,
) -> Result<Sources, String> {
    let devices = mac::input_devices().map_err(|err| err.to_string())?;
    let default = mac::default_input_uid();
    let saved = settings.get().input_device;
    let microphone = super::core::pick_microphone(&devices, saved.as_deref(), default.as_deref())
        .map(|d| d.name.clone());
    Ok(Sources {
        microphone,
        computer_audio: recorder.last_computer_audio(),
    })
}

#[tauri::command]
pub fn start_recording(app: AppHandle) -> Result<Recording, String> {
    start(&app, Source::Manual)
}

/// Starts recording for Record or for the meeting prompt, and tells the
/// window with a `recording-started` event, since the prompt may be
/// answered in a notification. Every way to start reads the saved input
/// device and quality here, once.
pub fn start(app: &AppHandle, source: Source) -> Result<Recording, String> {
    // The same folder the library lists, so a new recording shows up there.
    let notes_dir = app.state::<Library>().notes_dir();
    let microphone = crate::setup::mac::microphone_access();
    if !can_record(notes_dir.is_some(), microphone) {
        return Err(match notes_dir {
            None => "Choose a notes folder before recording.".into(),
            Some(_) => "Anchovy needs microphone access to record.".into(),
        });
    }
    let notes_dir = notes_dir.unwrap_or_default();
    let progress = app.clone();
    let recording = app
        .state::<Arc<Recorder>>()
        .start(
            &notes_dir,
            mac::local_now(),
            source,
            app.state::<Arc<SettingsStore>>().recording_options(),
            mac::start,
            PROGRESS_INTERVAL,
            move |p| {
                let _ = progress.emit(PROGRESS_EVENT, p);
            },
        )
        .map_err(|err| err.to_string())?;
    let _ = app.emit(STARTED_EVENT, &recording);
    Ok(recording)
}

/// Stops recording. With Generate notes automatically on, the note starts
/// right away; off, the recording stays Saved until Generate note.
#[tauri::command]
pub fn stop_recording(
    recorder: State<Arc<Recorder>>,
    pipeline: State<Arc<Pipeline>>,
    settings: State<Arc<SettingsStore>>,
) -> Result<Saved, String> {
    let saved = recorder.stop().map_err(|err| err.to_string())?;
    let automatic = settings.get().generate_notes_automatically;
    if let Err(err) = pipeline.after_recording(&saved.folder, automatic) {
        // The recording is saved either way; its note can be generated later.
        eprintln!("Anchovy couldn't start the note. {err}");
    }
    Ok(saved)
}
