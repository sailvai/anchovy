//! A recording's status and `.anchovy/state.json`, which stores it.

use std::fs;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::note::Source;
use super::{write_atomically, NotesError, APP_DIR};

pub const STATE_FILE: &str = "state.json";
const STATE_TMP_FILE: &str = "state.tmp";

/// Where a recording is on its way to a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    Recording,
    Saved,
    /// The selected models are not on this Mac yet.
    NeedsModels,
    Working,
    /// `note.md` is written.
    Ready,
    /// A step failed. `reason` is shown to the user next to Retry.
    Failed {
        reason: String,
    },
}

impl Status {
    pub fn name(&self) -> &'static str {
        match self {
            Status::Recording => "Recording",
            Status::Saved => "Saved",
            Status::NeedsModels => "Needs models",
            Status::Working => "Working",
            Status::Ready => "Ready",
            Status::Failed { .. } => "Failed",
        }
    }

    pub fn can_move_to(&self, next: &Status) -> bool {
        use Status::*;
        matches!(
            (self, next),
            (Recording, Saved)
                | (Saved, NeedsModels)
                | (Saved, Working)
                | (NeedsModels, Working)
                | (Working, Ready)
                | (Working, Failed { .. })
                | (Failed { .. }, Working)
                | (Ready, Working)
        )
    }
}

/// A source that reached the recording. Written as note.md's `inputs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Input {
    #[serde(rename = "microphone")]
    Microphone,
    #[serde(rename = "computer audio")]
    ComputerAudio,
}

/// Contents of `.anchovy/state.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(flatten)]
    pub status: Status,
    /// Models used for the current note, set when a note is generated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asr_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_model: Option<String>,
    /// What the recording actually captured. Empty only in folders written
    /// before recording existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<Input>,
    /// Record or the meeting prompt. Folders written before the prompt
    /// existed have none and were started by Record.
    #[serde(default, skip_serializing_if = "Source::is_manual")]
    pub source: Source,
    /// Why a Small recording could not be saved as M4A. Its audio was kept
    /// as `audio.wav` instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding_failed: Option<String>,
}

impl State {
    /// A recording that has just started.
    pub fn new() -> Self {
        State {
            status: Status::Recording,
            asr_model: None,
            summary_model: None,
            inputs: Vec::new(),
            source: Source::Manual,
            encoding_failed: None,
        }
    }

    /// Moves to `next`, or returns `InvalidTransition` and leaves the state
    /// unchanged if the move is not allowed.
    pub fn move_to(&mut self, next: Status) -> Result<(), NotesError> {
        if !self.status.can_move_to(&next) {
            return Err(NotesError::InvalidTransition {
                from: self.status.clone(),
                to: next,
            });
        }
        self.status = next;
        Ok(())
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

/// Reads `.anchovy/state.json` from a recording folder.
pub fn read_state(recording_dir: &Path) -> Result<State, NotesError> {
    let text = fs::read_to_string(recording_dir.join(APP_DIR).join(STATE_FILE))?;
    Ok(serde_json::from_str(&text)?)
}

/// Writes `.anchovy/state.json` in a recording folder, atomically.
pub fn write_state(recording_dir: &Path, state: &State) -> Result<(), NotesError> {
    let json = serde_json::to_vec_pretty(state)?;
    let app_dir = recording_dir.join(APP_DIR);
    fs::create_dir_all(&app_dir)?;
    write_atomically(
        &app_dir.join(STATE_TMP_FILE),
        &app_dir.join(STATE_FILE),
        |file| file.write_all(&json),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;

    fn failed() -> Status {
        Status::Failed {
            reason: "Not enough memory".into(),
        }
    }

    fn all() -> Vec<Status> {
        vec![
            Status::Recording,
            Status::Saved,
            Status::NeedsModels,
            Status::Working,
            Status::Ready,
            failed(),
        ]
    }

    #[test]
    fn allowed_transitions_follow_the_plan() {
        let allowed = [
            (Status::Recording, Status::Saved),
            (Status::Saved, Status::NeedsModels),
            (Status::Saved, Status::Working),
            (Status::NeedsModels, Status::Working),
            (Status::Working, Status::Ready),
            (Status::Working, failed()),
            (failed(), Status::Working),
            (Status::Ready, Status::Working),
        ];
        for from in all() {
            for to in all() {
                let expected = allowed.contains(&(from.clone(), to.clone()));
                assert_eq!(
                    from.can_move_to(&to),
                    expected,
                    "{} -> {}",
                    from.name(),
                    to.name()
                );
            }
        }
    }

    #[test]
    fn a_recording_starts_in_recording() {
        assert_eq!(State::new().status, Status::Recording);
    }

    #[test]
    fn allowed_moves_change_the_status() {
        let mut state = State::new();
        state.move_to(Status::Saved).unwrap();
        state.move_to(Status::Working).unwrap();
        state.move_to(failed()).unwrap();
        state.move_to(Status::Working).unwrap();
        state.move_to(Status::Ready).unwrap();
        assert_eq!(state.status, Status::Ready);
    }

    #[test]
    fn disallowed_moves_are_rejected_and_change_nothing() {
        let mut state = State::new();
        let before = state.clone();

        let err = state.move_to(Status::Ready).unwrap_err();

        assert!(matches!(
            err,
            NotesError::InvalidTransition {
                from: Status::Recording,
                to: Status::Ready,
            }
        ));
        assert_eq!(err.to_string(), "cannot move from Recording to Ready");
        assert_eq!(state, before);
    }

    #[test]
    fn state_json_round_trips_in_the_hidden_folder() {
        let dir = TestDir::new();
        let state = State {
            status: failed(),
            asr_model: Some("Qwen3-ASR 1.7B".into()),
            summary_model: Some("Qwen3-4B-Instruct-2507".into()),
            inputs: vec![Input::Microphone],
            source: Source::Manual,
            encoding_failed: Some("Disk full.".into()),
        };

        write_state(dir.path(), &state).unwrap();

        let path = dir.path().join(APP_DIR).join(STATE_FILE);
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(json["status"], "failed");
        assert_eq!(json["reason"], "Not enough memory");
        assert_eq!(json["asr_model"], "Qwen3-ASR 1.7B");
        assert_eq!(read_state(dir.path()).unwrap(), state);
        assert!(!dir.path().join(APP_DIR).join("state.tmp").exists());
    }

    #[test]
    fn status_names_in_state_json_are_snake_case() {
        let json = serde_json::to_value(State {
            status: Status::NeedsModels,
            asr_model: None,
            summary_model: None,
            inputs: Vec::new(),
            source: Source::Manual,
            encoding_failed: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "status": "needs_models" }));
    }

    #[test]
    fn inputs_are_written_as_in_note_md() {
        let mut state = State::new();
        state.inputs = vec![Input::Microphone, Input::ComputerAudio];
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "status": "recording",
                "inputs": ["microphone", "computer audio"]
            })
        );
        let back: State = serde_json::from_value(json).unwrap();
        assert_eq!(back, state);
    }

    #[test]
    fn source_is_kept_and_older_state_json_reads_as_manual() {
        let mut state = State::new();
        state.source = Source::Meeting;
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["source"], "meeting");
        assert_eq!(serde_json::from_value::<State>(json).unwrap(), state);

        // Before the meeting prompt, every recording was started by Record.
        let older: State = serde_json::from_str(r#"{ "status": "saved" }"#).unwrap();
        assert_eq!(older.source, Source::Manual);
    }

    #[test]
    fn unknown_status_in_state_json_is_an_error() {
        let dir = TestDir::new();
        std::fs::create_dir(dir.path().join(APP_DIR)).unwrap();
        std::fs::write(
            dir.path().join(APP_DIR).join(STATE_FILE),
            r#"{"status":"uploaded"}"#,
        )
        .unwrap();

        assert!(matches!(read_state(dir.path()), Err(NotesError::Json(_))));
    }
}
