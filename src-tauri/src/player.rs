//! What the note's player may open: the recording's `audio.wav` or
//! `audio.m4a`, through Tauri's asset protocol, and nothing outside the notes
//! folder.
//!
//! The protocol's static scope in `tauri.conf.json` is empty. At runtime the
//! current notes folder is allowed, recursively; when the notes folder
//! changes, the old one is forbidden and the new one allowed. Tauri never
//! serves files whose path has a part starting with `.`, so `.anchovy/` stays
//! closed. The interface only ever gets a path built here from a recording
//! folder name that `Library::path_of` accepted.
//!
//! Tauri's scope can add patterns but never remove them, and a forbidden
//! pattern always wins. So a notes folder inside the old one, or a folder used
//! earlier in the same run, cannot be allowed again until Anchovy restarts.
//! The player then says so instead of opening anything.

pub mod commands;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::library::Library;

/// The asset protocol's scope, as far as the player uses it.
pub trait AssetScope {
    fn allow_directory(&self, dir: &Path) -> Result<(), String>;
    fn forbid_directory(&self, dir: &Path) -> Result<(), String>;
    fn is_allowed(&self, path: &Path) -> bool;
}

impl AssetScope for tauri::scope::fs::Scope {
    fn allow_directory(&self, dir: &Path) -> Result<(), String> {
        tauri::scope::fs::Scope::allow_directory(self, dir, true).map_err(|err| err.to_string())
    }

    fn forbid_directory(&self, dir: &Path) -> Result<(), String> {
        tauri::scope::fs::Scope::forbid_directory(self, dir, true).map_err(|err| err.to_string())
    }

    fn is_allowed(&self, path: &Path) -> bool {
        tauri::scope::fs::Scope::is_allowed(self, path)
    }
}

/// Moves the scope from the `old` notes folder, if any, to `new`.
/// Moves the scope from the `old` notes folder, if any, to `new`. An old
/// folder inside the new one is part of it and stays open.
pub fn follow_notes_folder(
    scope: &impl AssetScope,
    old: Option<&Path>,
    new: &Path,
) -> Result<(), String> {
    // Tauri matches allowed folders as given but canonicalizes each path it
    // is asked for, so the folders must be canonical too.
    let new = canonical(new)?;
    if let Some(old) = old.map(canonical).transpose()? {
        if !old.starts_with(&new) {
            scope.forbid_directory(&old)?;
        }
    }
    scope.allow_directory(&new)
}

fn canonical(dir: &Path) -> Result<PathBuf, String> {
    dir.canonicalize()
        .map_err(|err| format!("{}: {err}", dir.display()))
}

/// A recording's audio, for the player.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordingAudio {
    pub path: PathBuf,
    /// `audio.wav` or `audio.m4a`.
    pub name: String,
}

/// Shown when the notes folder changed in a way the scope cannot follow.
pub const RESTART_TO_PLAY: &str =
    "Quit and reopen Anchovy to play recordings in this notes folder.";

/// The audio of recording `folder`, if it has any, and only if the asset
/// protocol will serve it.
pub fn recording_audio(
    library: &Library,
    scope: &impl AssetScope,
    folder: &str,
) -> Result<Option<RecordingAudio>, String> {
    let Some(path) = library.audio_of(folder).map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    if !scope.is_allowed(&path) {
        return Err(RESTART_TO_PLAY.into());
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Some(RecordingAudio { path, name }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;
    use std::fs;
    use tauri::scope::fs::Scope;
    use tauri::test::{mock_app, MockRuntime};
    use tauri::utils::config::FsScope;

    /// Tauri's own scope, as the asset protocol checks it, starting empty
    /// like the static scope in tauri.conf.json.
    fn empty_scope() -> (tauri::App<MockRuntime>, Scope) {
        let app = mock_app();
        let scope = Scope::new(&app, &FsScope::default()).unwrap();
        (app, scope)
    }

    fn file(path: &Path) -> PathBuf {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"audio").unwrap();
        path.to_path_buf()
    }

    #[test]
    fn the_scope_allows_the_notes_folder_and_nothing_outside_it() {
        let dir = TestDir::new();
        let notes = dir.path().join("Notes");
        let audio = file(&notes.join("2026-09-26-1410/audio.wav"));
        let outside = file(&dir.path().join("Elsewhere/secret.wav"));
        let hidden = file(&notes.join("2026-09-26-1410/.anchovy/recording.wav"));
        let (_app, scope) = empty_scope();
        assert!(!scope.is_allowed(&audio), "the static scope is empty");

        follow_notes_folder(&scope, None, &notes).unwrap();

        assert!(scope.is_allowed(&audio));
        assert!(!scope.is_allowed(&outside));
        assert!(!scope.is_allowed(notes.join("2026-09-26-1410/../../Elsewhere/secret.wav")));
        assert!(!scope.is_allowed(notes.join("../Elsewhere/secret.wav")));
        assert!(!scope.is_allowed(&hidden), "app state stays closed");
    }

    #[test]
    fn the_scope_moves_when_the_notes_folder_changes() {
        let dir = TestDir::new();
        let old = dir.path().join("Documents/Anchovy");
        let new = dir.path().join("Vault");
        let old_audio = file(&old.join("2026-09-26-1410/audio.wav"));
        let new_audio = file(&new.join("2026-09-27-0900/audio.m4a"));
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &old).unwrap();

        follow_notes_folder(&scope, Some(&old), &new).unwrap();

        assert!(!scope.is_allowed(&old_audio));
        assert!(scope.is_allowed(&new_audio));
    }

    #[test]
    fn choosing_the_same_folder_again_keeps_it_allowed() {
        let dir = TestDir::new();
        let notes = dir.path().join("Notes");
        let audio = file(&notes.join("2026-09-26-1410/audio.wav"));
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &notes).unwrap();

        follow_notes_folder(&scope, Some(&notes), &notes).unwrap();

        assert!(scope.is_allowed(&audio));
    }

    #[test]
    fn an_old_folder_inside_the_new_one_stays_open_as_part_of_it() {
        let dir = TestDir::new();
        let old = dir.path().join("Vault/Meetings");
        let new = dir.path().join("Vault");
        let old_audio = file(&old.join("2026-09-26-1410/audio.wav"));
        let new_audio = file(&new.join("2026-09-27-0900/audio.wav"));
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &old).unwrap();

        follow_notes_folder(&scope, Some(&old), &new).unwrap();

        assert!(scope.is_allowed(&old_audio));
        assert!(scope.is_allowed(&new_audio));
    }

    #[test]
    fn a_new_folder_inside_the_old_one_stays_closed_until_restart() {
        // Forbidding the old folder must win over allowing a folder inside
        // it: the rest of the old folder is outside the notes folder now.
        let dir = TestDir::new();
        let old = dir.path().join("Vault");
        let new = dir.path().join("Vault/Meetings");
        let sibling = file(&old.join("Private/secret.wav"));
        let inside = file(&new.join("2026-09-27-0900/audio.wav"));
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &old).unwrap();

        follow_notes_folder(&scope, Some(&old), &new).unwrap();

        assert!(!scope.is_allowed(&sibling));
        assert!(!scope.is_allowed(&inside));
    }

    #[test]
    fn the_config_turns_the_asset_protocol_on_with_an_empty_static_scope() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let security = &config["app"]["security"];
        assert_eq!(
            security["assetProtocol"],
            serde_json::json!({ "enable": true, "scope": [] })
        );
        let csp = security["csp"].as_str().unwrap();
        let media: Vec<&str> = csp
            .split(';')
            .map(str::trim)
            .filter(|directive| directive.starts_with("media-src"))
            .collect();
        assert_eq!(media, ["media-src 'self' asset: http://asset.localhost"]);
    }

    /// A notes folder with one recording of each quality and one without
    /// audio yet.
    fn library(dir: &TestDir) -> (Library, PathBuf) {
        let notes = dir.path().join("Notes");
        file(&notes.join("2026-09-26-1410/audio.wav"));
        file(&notes.join("2026-09-26-1500/audio.m4a"));
        file(&notes.join("2026-09-26-1600/.anchovy/recording.wav"));
        file(&dir.path().join("Elsewhere/audio.wav"));
        (Library::new(notes.clone()), notes)
    }

    #[test]
    fn the_player_gets_audio_wav_or_audio_m4a_from_the_notes_folder() {
        let dir = TestDir::new();
        let (library, notes) = library(&dir);
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &notes).unwrap();

        assert_eq!(
            recording_audio(&library, &scope, "2026-09-26-1410").unwrap(),
            Some(RecordingAudio {
                path: notes.join("2026-09-26-1410/audio.wav"),
                name: "audio.wav".into(),
            })
        );
        assert_eq!(
            recording_audio(&library, &scope, "2026-09-26-1500").unwrap(),
            Some(RecordingAudio {
                path: notes.join("2026-09-26-1500/audio.m4a"),
                name: "audio.m4a".into(),
            })
        );
        // Recording Small: only the hidden WAV, which is not offered.
        assert_eq!(
            recording_audio(&library, &scope, "2026-09-26-1600").unwrap(),
            None
        );
    }

    #[test]
    fn no_folder_name_reaches_outside_the_notes_folder() {
        let dir = TestDir::new();
        let (library, notes) = library(&dir);
        let (_app, scope) = empty_scope();
        follow_notes_folder(&scope, None, &notes).unwrap();

        for folder in [
            "../Elsewhere",
            "2026-09-26-1410/../../Elsewhere",
            "/etc",
            "Elsewhere",
            "2026-09-26-1410/.anchovy",
            "",
        ] {
            assert!(
                recording_audio(&library, &scope, folder).is_err(),
                "{folder}"
            );
        }
    }

    #[test]
    fn audio_the_scope_does_not_serve_is_not_offered() {
        let dir = TestDir::new();
        let (library, _notes) = library(&dir);
        let (_app, scope) = empty_scope();

        assert_eq!(
            recording_audio(&library, &scope, "2026-09-26-1410").unwrap_err(),
            RESTART_TO_PLAY
        );
    }
}
