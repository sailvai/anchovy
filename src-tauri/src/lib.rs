//! Anchovy's Rust core: the window, the notes module (recording folders,
//! state, and note.md), the library (the recording list), the models module
//! (the shipped list and downloads), the recording module, and first launch
//! (the notes folder bookmark and permissions). Inference arrives in a later
//! plan step.

pub mod folder_access;
pub mod library;
pub mod models;
pub mod notes;
pub mod recording;
pub mod setup;

use folder_access::{commands as folder_commands, mac as folder_mac, FolderAccess};
use library::{commands as library_commands, Library};
use models::catalog::Catalog;
use models::store::Store;
use models::{commands as model_commands, mac as model_mac, Models};
use recording::commands as recording_commands;
use recording::core::Recorder;
use serde::Serialize;
use setup::{commands as setup_commands, Setup};
use std::sync::Arc;

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
    let models = Models::new(Catalog::shipped(), Store::new(models_dir), memory);
    let support_dir = folder_mac::support_dir().expect("HOME is set for every macOS app");
    let folder_access = FolderAccess::new(support_dir.clone(), folder_mac::MacBookmarks);
    // Until a notes folder is chosen on first launch, the library is empty.
    let library = Library::without_folder();
    match folder_access.restore() {
        Ok(Some(notes_dir)) => library.set_notes_dir(notes_dir),
        Ok(None) => {}
        Err(err) => eprintln!("Anchovy can't read the saved notes folder. {err}"),
    }
    tauri::Builder::default()
        .manage(Arc::new(models))
        .manage(library)
        .manage(folder_access)
        .manage(Arc::new(Setup::load(support_dir)))
        .manage(Arc::new(Recorder::new()))
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
            folder_commands::choose_notes_folder,
            folder_commands::use_default_notes_folder,
            setup_commands::setup_status,
            setup_commands::request_microphone,
            setup_commands::check_computer_audio,
            setup_commands::finish_setup,
            setup_commands::open_privacy_settings,
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
