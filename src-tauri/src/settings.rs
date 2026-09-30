//! The four settings: the notes folder, the input device, the recording
//! quality, and whether notes are generated automatically.
//!
//! Three of them live in `settings.json` in the app's support folder. The
//! notes folder is not copied there: it stays the security-scoped bookmark
//! that `folder_access` saves, because only the bookmark reopens it inside the
//! sandbox.
//!
//! A recording reads the input device and quality once, when it starts
//! ([`SettingsStore::recording_options`]), so a change applies to the next
//! recording only.

pub mod commands;

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::notes::folder::Quality;
use crate::notes::write_atomically;

pub const SETTINGS_FILE: &str = "settings.json";
const SETTINGS_TMP_FILE: &str = "settings.tmp";

/// Contents of `settings.json`. Every field has a default, so a file written
/// by an older version, or with a field missing, still loads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// UID of the chosen input device, or `None` for the system default. A
    /// device that is not connected is kept here; recording falls back to the
    /// system default until it is back.
    pub input_device: Option<String>,
    pub recording_quality: Quality,
    /// Start a note as soon as a recording stops. On by default.
    pub generate_notes_automatically: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            input_device: None,
            recording_quality: Quality::High,
            generate_notes_automatically: true,
        }
    }
}

/// What a recording is made with, read once when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordingOptions {
    pub input_device: Option<String>,
    pub quality: Quality,
}

/// Reads `settings.json` from the app's support folder. A missing or
/// unreadable file means the defaults.
pub fn read_settings(dir: &Path) -> Settings {
    fs::read(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

/// Writes `settings.json` atomically: on any failure the old file is kept.
pub fn write_settings(dir: &Path, settings: &Settings) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(settings)?;
    fs::create_dir_all(dir)?;
    write_atomically(
        &dir.join(SETTINGS_TMP_FILE),
        &dir.join(SETTINGS_FILE),
        |file| file.write_all(&json),
    )
}

/// The settings in memory, saved to `settings.json` on every change.
pub struct SettingsStore {
    dir: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Loads the settings saved in `dir`, the app's support folder.
    pub fn load(dir: PathBuf) -> Self {
        let current = Mutex::new(read_settings(&dir));
        SettingsStore { dir, current }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    /// Saves `next`, then uses it. If saving fails, nothing changes.
    pub fn update(&self, next: Settings) -> io::Result<Settings> {
        let mut current = self.current.lock().unwrap();
        write_settings(&self.dir, &next)?;
        *current = next.clone();
        Ok(next)
    }

    /// The input device and quality for a recording that starts now.
    pub fn recording_options(&self) -> RecordingOptions {
        let settings = self.current.lock().unwrap();
        RecordingOptions {
            input_device: settings.input_device.clone(),
            quality: settings.recording_quality,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;

    fn small_on_usb() -> Settings {
        Settings {
            input_device: Some("AppleUSBAudioEngine:DJI".into()),
            recording_quality: Quality::Small,
            generate_notes_automatically: false,
        }
    }

    #[test]
    fn settings_json_round_trips_in_the_support_folder() {
        let dir = TestDir::new();

        write_settings(dir.path(), &small_on_usb()).unwrap();

        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join(SETTINGS_FILE)).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "input_device": "AppleUSBAudioEngine:DJI",
                "recording_quality": "small",
                "generate_notes_automatically": false,
            })
        );
        assert_eq!(read_settings(dir.path()), small_on_usb());
        assert!(!dir.path().join(SETTINGS_TMP_FILE).exists());
    }

    #[test]
    fn a_file_from_plan_step_6b_loads_with_the_new_defaults() {
        let dir = TestDir::new();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"generate_notes_automatically": false}"#,
        )
        .unwrap();

        assert_eq!(
            read_settings(dir.path()),
            Settings {
                input_device: None,
                recording_quality: Quality::High,
                generate_notes_automatically: false,
            }
        );
    }

    #[test]
    fn a_missing_or_unreadable_file_means_the_defaults() {
        let dir = TestDir::new();
        let defaults = Settings {
            input_device: None,
            recording_quality: Quality::High,
            generate_notes_automatically: true,
        };
        assert_eq!(read_settings(dir.path()), defaults);
        for broken in ["not json", r#"{"recording_quality": "medium"}"#, "[]"] {
            fs::write(dir.path().join(SETTINGS_FILE), broken).unwrap();
            assert_eq!(read_settings(dir.path()), defaults, "{broken}");
        }
    }

    #[test]
    fn automatic_notes_are_on_unless_turned_off() {
        // Moved from pipeline.rs (plan step 6b) with `read_settings`.
        let dir = TestDir::new();
        assert!(read_settings(dir.path()).generate_notes_automatically);
        fs::write(dir.path().join(SETTINGS_FILE), "not json").unwrap();
        assert!(read_settings(dir.path()).generate_notes_automatically);
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"generate_notes_automatically": false}"#,
        )
        .unwrap();
        assert!(!read_settings(dir.path()).generate_notes_automatically);
    }

    #[test]
    fn the_store_saves_each_change_and_loads_it_next_launch() {
        let dir = TestDir::new();
        let store = SettingsStore::load(dir.path().to_path_buf());
        assert_eq!(store.get(), Settings::default());

        assert_eq!(store.update(small_on_usb()).unwrap(), small_on_usb());

        assert_eq!(store.get(), small_on_usb());
        let next_launch = SettingsStore::load(dir.path().to_path_buf());
        assert_eq!(next_launch.get(), small_on_usb());
    }

    #[test]
    fn a_change_that_cannot_be_saved_changes_nothing() {
        let dir = TestDir::new();
        // A folder where the file should be: the rename over it fails.
        fs::create_dir(dir.path().join(SETTINGS_FILE)).unwrap();
        let store = SettingsStore::load(dir.path().to_path_buf());

        assert!(store.update(small_on_usb()).is_err());

        assert_eq!(store.get(), Settings::default());
        assert!(!dir.path().join(SETTINGS_TMP_FILE).exists());
    }

    #[test]
    fn a_recording_reads_the_device_and_quality() {
        let dir = TestDir::new();
        let store = SettingsStore::load(dir.path().to_path_buf());
        assert_eq!(
            store.recording_options(),
            RecordingOptions {
                input_device: None,
                quality: Quality::High,
            }
        );
        store.update(small_on_usb()).unwrap();
        assert_eq!(
            store.recording_options(),
            RecordingOptions {
                input_device: Some("AppleUSBAudioEngine:DJI".into()),
                quality: Quality::Small,
            }
        );
    }
}
