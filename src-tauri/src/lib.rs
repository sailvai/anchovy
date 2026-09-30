//! Anchovy's Rust core: the window, the notes module (recording folders,
//! state, and note.md), the library (the recording list), the models module
//! (the shipped list and downloads), the recording module, first launch
//! (the notes folder bookmark and permissions), the engines that run the
//! models, the pipeline from a recording to its note, the meeting prompt,
//! and the settings.

pub mod engines;
pub mod folder_access;
pub mod library;
pub mod m4a;
pub mod meetings;
pub mod models;
pub mod notes;
pub mod pipeline;
pub mod recording;
pub mod settings;
pub mod setup;

use engines::llama::LlamaEngines;
use engines::mac::MacMemory;
use folder_access::{commands as folder_commands, mac as folder_mac, FolderAccess};
use library::{commands as library_commands, Library};
use meetings::commands::{self as meeting_commands, Meetings};
use models::catalog::Catalog;
use models::store::Store;
use models::{commands as model_commands, mac as model_mac, Models};
use pipeline::{commands as note_commands, Deps, Pipeline, MEMORY_WAIT};
use recording::commands as recording_commands;
use recording::core::Recorder;
use serde::Serialize;
use settings::{commands as settings_commands, SettingsStore};
use setup::{commands as setup_commands, Setup};
use std::sync::Arc;
use tauri::Manager;

#[derive(Debug, PartialEq, Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        name: "Anchovy",
        version: env!("CARGO_PKG_VERSION"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let models_dir = model_mac::models_dir().expect("HOME is set for every macOS app");
    // hw.memsize does not fail on macOS; 8 GB is the smallest Apple Silicon Mac.
    let memory = model_mac::memory_bytes().unwrap_or(8 * models::fit::GIB);
    let models = Arc::new(Models::new(
        Catalog::shipped(),
        Store::new(models_dir),
        memory,
    ));
    let support_dir = folder_mac::support_dir().expect("HOME is set for every macOS app");
    let folder_access = FolderAccess::new(support_dir.clone(), folder_mac::MacBookmarks);
    // Until a notes folder is chosen on first launch, the library is empty.
    let library = Library::without_folder();
    match folder_access.restore() {
        Ok(Some(notes_dir)) => library.set_notes_dir(notes_dir),
        Ok(None) => {}
        Err(err) => eprintln!("Anchovy can't read the saved notes folder. {err}"),
    }
    let settings = Arc::new(SettingsStore::load(support_dir.clone()));
    let deps = Deps {
        models: models.clone(),
        engines: Arc::new(LlamaEngines),
        memory: Arc::new(MacMemory),
        chunk_tokens: engines::summary::chunk_tokens(memory),
        memory_wait: MEMORY_WAIT,
    };
    tauri::Builder::default()
        .setup(move |app| {
            let pipeline = Pipeline::new(deps, note_commands::emitter(app.handle().clone()));
            let folders = note_commands::recording_folders(&app.state::<Library>());
            // A note left Working by an app that quit will never finish.
            pipeline.recover(&folders);
            pipeline.resume_waiting(&folders);
            let pipeline = Arc::new(pipeline);
            app.manage(pipeline.clone());
            // A Small recording whose app was killed left its WAV in
            // `.anchovy/`: encode it now, as Stop would have.
            std::thread::spawn(move || {
                for folder in &folders {
                    match recording::small::recover(folder, &m4a::mac::MacEncoder) {
                        Ok(Some(_)) => pipeline.changed(folder),
                        Ok(None) => {}
                        Err(err) => eprintln!(
                            "Anchovy couldn't finish the recording in {}. {err}",
                            folder.display()
                        ),
                    }
                }
            });
            meeting_commands::watch(app.handle().clone());
            Ok(())
        })
        .manage(models)
        .manage(settings)
        .manage(library)
        .manage(folder_access)
        .manage(Arc::new(Setup::load(support_dir)))
        .manage(Arc::new(Recorder::new(Arc::new(m4a::mac::MacEncoder))))
        .manage(Arc::new(Meetings::new()))
        .invoke_handler(tauri::generate_handler![
            app_info,
            library_commands::list_recordings,
            library_commands::read_note,
            library_commands::show_in_finder,
            library_commands::move_to_trash,
            model_commands::list_models,
            model_commands::select_model,
            model_commands::download_model,
            model_commands::cancel_model_download,
            model_commands::delete_model,
            recording_commands::list_input_devices,
            recording_commands::recording_sources,
            recording_commands::start_recording,
            recording_commands::stop_recording,
            note_commands::generate_note,
            note_commands::note_progress,
            note_commands::resume_waiting_notes,
            folder_commands::choose_notes_folder,
            folder_commands::use_default_notes_folder,
            setup_commands::setup_status,
            setup_commands::request_microphone,
            setup_commands::check_computer_audio,
            setup_commands::finish_setup,
            setup_commands::open_privacy_settings,
            meeting_commands::meeting_prompt,
            meeting_commands::answer_meeting_prompt,
            settings_commands::get_settings,
            settings_commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Anchovy");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_names_the_app_and_its_version() {
        assert_eq!(
            app_info(),
            AppInfo {
                name: "Anchovy",
                version: "0.1.0",
            }
        );
    }

    #[test]
    fn app_info_serializes_for_the_interface() {
        let json = serde_json::to_value(app_info()).unwrap();
        assert_eq!(json["name"], "Anchovy");
        assert_eq!(json["version"], "0.1.0");
    }
}
