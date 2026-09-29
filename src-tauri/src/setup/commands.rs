//! Tauri commands for the first-launch screens and the sources rows. Thin
//! wrappers over `Setup`, `setup::mac`, and the computer audio probe.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, State};

use super::mac::{self, PrivacyPane};
use super::{can_record, Access, Setup, SetupStatus};
use crate::folder_access::{default_folder, describe, mac::user_home};
use crate::library::Library;
use crate::recording::core::Recorder;

/// How long the computer audio check listens.
const PROBE_DURATION: Duration = Duration::from_secs(1);

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub fn setup_status(
    library: State<Library>,
    setup: State<Arc<Setup>>,
) -> Result<SetupStatus, String> {
    let home = user_home().map_err(|err| err.to_string())?;
    let notes_folder = library.notes_dir().map(|folder| describe(&folder, &home));
    let microphone = mac::microphone_access();
    Ok(SetupStatus {
        can_record: can_record(notes_folder.is_some(), microphone),
        notes_folder,
        default_folder: describe(&default_folder(&home), &home),
        microphone,
        computer_audio: setup.computer_audio(),
        finished: setup.state().finished,
    })
}

/// Shows the microphone prompt if it has not been shown, and returns the
/// answer.
#[tauri::command]
pub async fn request_microphone() -> Result<Access, String> {
    blocking(mac::request_microphone).await
}

/// Checks computer audio with a one-second silent probe. With `ask`, this
/// is the Allow button: the first probe makes macOS show its prompt. Without
/// `ask`, nothing is probed until the user has been asked once.
#[tauri::command]
pub async fn check_computer_audio(
    setup: State<'_, Arc<Setup>>,
    recorder: State<'_, Arc<Recorder>>,
    ask: bool,
) -> Result<Access, String> {
    if !ask && !setup.state().computer_audio_asked {
        return Ok(Access::NotAsked);
    }
    // The recording already shows what it gets.
    if recorder.is_recording() {
        return Ok(setup.computer_audio());
    }
    setup
        .mark_computer_audio_asked()
        .map_err(|err| err.to_string())?;
    let heard = blocking(|| crate::recording::mac::probe_computer_audio(PROBE_DURATION))
        .await?
        .map_err(|err| err.to_string())?;
    setup.record_probe(heard);
    Ok(setup.computer_audio())
}

/// Continue or Later on the last first-launch screen.
#[tauri::command]
pub fn finish_setup(setup: State<Arc<Setup>>) -> Result<(), String> {
    setup.finish().map_err(|err| err.to_string())
}

/// `pane` is `microphone` or `computer_audio`.
#[tauri::command]
pub fn open_privacy_settings(app: AppHandle, pane: String) -> Result<(), String> {
    let pane = match pane.as_str() {
        "microphone" => PrivacyPane::Microphone,
        "computer_audio" => PrivacyPane::ComputerAudio,
        _ => return Err(format!("No settings pane named {pane}.")),
    };
    // AppKit wants the main thread.
    app.run_on_main_thread(move || mac::open_privacy_settings(pane))
        .map_err(|err| err.to_string())
}
