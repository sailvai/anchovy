//! First launch: choose a notes folder, allow the microphone and computer
//! audio, download the default models. This module keeps what has been done
//! across launches and decides when Record is available: once there is a
//! notes folder and the microphone is allowed. Models can come later.
//!
//! macOS has no public call that reports the computer audio permission, so
//! its state comes from a one-second probe (`recording::mac::probe_computer_audio`)
//! and is only checked after the user has been asked once.

pub mod commands;
pub mod mac;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::folder_access::NotesFolder;

/// Saved in the app's support folder.
pub const SETUP_FILE: &str = "setup.json";

/// A permission as the first-launch screen and the sources rows show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// The system prompt has not been shown yet.
    NotAsked,
    /// Asked before, but not checked yet in this launch.
    Unchecked,
    Allowed,
    Denied,
}

/// What survives a relaunch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetupState {
    /// The user went through all three screens (Continue or Later on the last).
    #[serde(default)]
    pub finished: bool,
    /// The computer audio prompt was triggered at least once.
    #[serde(default)]
    pub computer_audio_asked: bool,
}

/// Everything the interface needs to decide what to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SetupStatus {
    pub notes_folder: Option<NotesFolder>,
    pub default_folder: NotesFolder,
    pub microphone: Access,
    pub computer_audio: Access,
    pub finished: bool,
    pub can_record: bool,
}

/// Record needs somewhere to save and a microphone it may use. Computer
/// audio and models are optional: without them the recording is microphone
/// only, or waits for its note.
pub fn can_record(has_notes_folder: bool, microphone: Access) -> bool {
    has_notes_folder && microphone == Access::Allowed
}

/// The computer audio row, from whether the prompt was ever shown and what
/// the last probe in this launch heard.
pub fn computer_audio_access(asked: bool, heard: Option<bool>) -> Access {
    match (asked, heard) {
        (false, _) => Access::NotAsked,
        (true, None) => Access::Unchecked,
        (true, Some(true)) => Access::Allowed,
        (true, Some(false)) => Access::Denied,
    }
}

pub fn read_setup(dir: &Path) -> io::Result<SetupState> {
    match fs::read(dir.join(SETUP_FILE)) {
        Ok(data) => serde_json::from_slice(&data).map_err(io::Error::other),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(SetupState::default()),
        Err(err) => Err(err),
    }
}

pub fn write_setup(dir: &Path, state: &SetupState) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let data = serde_json::to_vec_pretty(state).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{SETUP_FILE}.tmp"));
    crate::notes::write_atomically(&tmp, &dir.join(SETUP_FILE), |file| {
        io::Write::write_all(file, &data)
    })
}

/// First-launch progress, shared by the commands.
pub struct Setup {
    dir: PathBuf,
    state: Mutex<SetupState>,
    /// What the last computer audio probe heard in this launch.
    heard: Mutex<Option<bool>>,
}

impl Setup {
    /// Loads saved progress from `dir`. Unreadable progress starts over, so
    /// the first-launch screens show again rather than the app failing.
    pub fn load(dir: PathBuf) -> Self {
        let state = read_setup(&dir).unwrap_or_default();
        Setup {
            dir,
            state: Mutex::new(state),
            heard: Mutex::new(None),
        }
    }

    fn update(&self, change: impl FnOnce(&mut SetupState)) -> io::Result<()> {
        let mut state = self.state.lock().unwrap();
        let mut next = state.clone();
        change(&mut next);
        write_setup(&self.dir, &next)?;
        *state = next;
        Ok(())
    }

    pub fn state(&self) -> SetupState {
        self.state.lock().unwrap().clone()
    }

    pub fn finish(&self) -> io::Result<()> {
        self.update(|state| state.finished = true)
    }

    /// Call before triggering the computer audio prompt.
    pub fn mark_computer_audio_asked(&self) -> io::Result<()> {
        if self.state().computer_audio_asked {
            return Ok(());
        }
        self.update(|state| state.computer_audio_asked = true)
    }

    pub fn record_probe(&self, heard: bool) {
        *self.heard.lock().unwrap() = Some(heard);
    }

    pub fn computer_audio(&self) -> Access {
        computer_audio_access(
            self.state().computer_audio_asked,
            *self.heard.lock().unwrap(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;

    #[test]
    fn record_needs_a_folder_and_the_microphone() {
        assert!(can_record(true, Access::Allowed));
        assert!(!can_record(false, Access::Allowed));
        for microphone in [Access::NotAsked, Access::Unchecked, Access::Denied] {
            assert!(!can_record(true, microphone), "{microphone:?}");
        }
    }

    #[test]
    fn computer_audio_is_only_known_after_asking_and_probing() {
        assert_eq!(computer_audio_access(false, None), Access::NotAsked);
        assert_eq!(computer_audio_access(true, None), Access::Unchecked);
        assert_eq!(computer_audio_access(true, Some(true)), Access::Allowed);
        // A silent probe after asking means the prompt was denied or is
        // still open; both mean the other side of a call is not recorded.
        assert_eq!(computer_audio_access(true, Some(false)), Access::Denied);
    }

    #[test]
    fn a_new_install_has_done_nothing_yet() {
        let dir = TestDir::new();
        assert_eq!(read_setup(dir.path()).unwrap(), SetupState::default());
        let setup = Setup::load(dir.path().join("missing"));
        assert!(!setup.state().finished);
        assert_eq!(setup.computer_audio(), Access::NotAsked);
    }

    #[test]
    fn progress_survives_a_relaunch() {
        let dir = TestDir::new();
        let setup = Setup::load(dir.path().join("support"));
        setup.mark_computer_audio_asked().unwrap();
        setup.record_probe(true);
        assert_eq!(setup.computer_audio(), Access::Allowed);
        setup.finish().unwrap();

        let relaunched = Setup::load(dir.path().join("support"));

        assert_eq!(
            relaunched.state(),
            SetupState {
                finished: true,
                computer_audio_asked: true,
            }
        );
        // Asked before, not yet probed in this launch.
        assert_eq!(relaunched.computer_audio(), Access::Unchecked);
    }

    #[test]
    fn unreadable_progress_starts_the_first_launch_over() {
        let dir = TestDir::new();
        fs::write(dir.path().join(SETUP_FILE), "{not json").unwrap();
        assert!(read_setup(dir.path()).is_err());
        assert_eq!(
            Setup::load(dir.path().to_path_buf()).state(),
            SetupState::default()
        );
    }

    #[test]
    fn status_serializes_for_the_interface() {
        let folder = NotesFolder {
            path: "/Users/someone/Documents/Anchovy".into(),
            display: "~/Documents/Anchovy".into(),
            exists: false,
            obsidian_vault: false,
        };
        let status = SetupStatus {
            notes_folder: None,
            default_folder: folder,
            microphone: Access::NotAsked,
            computer_audio: Access::Unchecked,
            finished: false,
            can_record: false,
        };
        let json = serde_json::to_value(status).unwrap();
        assert_eq!(json["microphone"], "not_asked");
        assert_eq!(json["computer_audio"], "unchecked");
        assert_eq!(json["default_folder"]["display"], "~/Documents/Anchovy");
        assert_eq!(json["notes_folder"], serde_json::Value::Null);
    }
}
