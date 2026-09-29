//! Anchovy's Rust core: the window, the notes module (recording folders,
//! state, and note.md), the library (the recording list), and the models
//! module (the shipped list and downloads). Recording and inference arrive in later plan steps.

pub mod library;
pub mod models;
pub mod notes;

use library::{commands as library_commands, mac as library_mac, Library};
use models::catalog::Catalog;
use models::store::Store;
use models::{commands as model_commands, mac as model_mac, Models};
use serde::Serialize;
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
    let notes_dir = library_mac::notes_dir().expect("HOME is set for every macOS app");
    tauri::Builder::default()
        .manage(Arc::new(models))
        .manage(Library::new(notes_dir))
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
