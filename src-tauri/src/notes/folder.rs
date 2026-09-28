//! Recording folder names and the audio file inside each folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Local wall-clock time a recording started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl StartTime {
    /// `yyyy-MM-dd-HHmm`, the base name of the recording folder.
    pub fn folder_name(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}-{:02}{:02}",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }
}

/// Recording quality, which decides the audio format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    High,
    Small,
}

impl Quality {
    pub fn audio_file_name(self) -> &'static str {
        match self {
            Quality::High => "audio.wav",
            Quality::Small => "audio.m4a",
        }
    }
}

/// The base name, then the base name with `-2`, `-3`, and so on.
fn candidates(start: &StartTime) -> impl Iterator<Item = String> {
    let base = start.folder_name();
    std::iter::once(base.clone()).chain((2..).map(move |n| format!("{base}-{n}")))
}

/// Creates the folder for a new recording inside `notes_dir` and returns its
/// path. If the start minute is taken, adds `-2`, `-3`, and so on; never
/// reuses an existing folder.
pub fn create_recording_folder(notes_dir: &Path, start: &StartTime) -> io::Result<PathBuf> {
    // `create_dir` fails if the name exists, so two recordings started in the
    // same minute can never end up sharing a folder.
    for name in candidates(start) {
        let path = notes_dir.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    unreachable!("candidate names never run out")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;

    const START: StartTime = StartTime {
        year: 2026,
        month: 9,
        day: 6,
        hour: 9,
        minute: 5,
        second: 30,
    };

    #[test]
    fn high_quality_is_wav_and_small_is_m4a() {
        assert_eq!(Quality::High.audio_file_name(), "audio.wav");
        assert_eq!(Quality::Small.audio_file_name(), "audio.m4a");
    }

    #[test]
    fn folder_is_named_by_start_minute_with_zero_padding() {
        assert_eq!(START.folder_name(), "2026-09-06-0905");
    }

    #[test]
    fn creating_folders_in_the_same_minute_never_reuses_one() {
        let dir = TestDir::new();

        let first = create_recording_folder(dir.path(), &START).unwrap();
        let second = create_recording_folder(dir.path(), &START).unwrap();
        let third = create_recording_folder(dir.path(), &START).unwrap();

        assert_eq!(first, dir.path().join("2026-09-06-0905"));
        assert_eq!(second, dir.path().join("2026-09-06-0905-2"));
        assert_eq!(third, dir.path().join("2026-09-06-0905-3"));
        assert!(first.is_dir() && second.is_dir() && third.is_dir());
    }

    #[test]
    fn a_file_with_the_folder_name_counts_as_taken() {
        let dir = TestDir::new();
        fs::write(dir.path().join("2026-09-06-0905"), "").unwrap();

        let created = create_recording_folder(dir.path(), &START).unwrap();

        assert_eq!(created, dir.path().join("2026-09-06-0905-2"));
    }
}
