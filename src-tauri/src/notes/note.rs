//! Builds `note.md` and writes it without ever leaving half a note.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::folder::{Quality, StartTime};
use super::{write_atomically, APP_DIR};

pub const NOTE_FILE: &str = "note.md";
pub const NOTE_TMP_FILE: &str = "note.tmp";

/// Headings are English in every note so other tools can find them.
pub const HEADINGS: [&str; 5] = [
    "Summary",
    "Decisions",
    "Action items",
    "Transcript",
    "Audio",
];

/// How the recording was started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// The user pressed Record.
    #[default]
    Manual,
    /// The user accepted the meeting prompt.
    Meeting,
}

impl Source {
    pub fn is_manual(&self) -> bool {
        *self == Source::Manual
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::Meeting => "meeting",
        }
    }
}

/// Everything that goes into one `note.md`. Body text stays in the language
/// spoken in the meeting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub start: StartTime,
    pub duration_seconds: u64,
    pub source: Source,
    /// Whether computer audio was actually recorded. The microphone always is.
    pub computer_audio: bool,
    pub quality: Quality,
    pub asr_model: String,
    pub summary_model: String,
    pub summary: String,
    pub decisions: Vec<String>,
    pub action_items: Vec<String>,
    pub transcript: String,
}

impl Note {
    /// The inputs that were actually recorded, as written in `inputs`.
    pub fn inputs(&self) -> Vec<&'static str> {
        if self.computer_audio {
            vec!["microphone", "computer audio"]
        } else {
            vec!["microphone"]
        }
    }

    /// The full `note.md` text: front matter, title, and the five sections.
    pub fn render(&self) -> String {
        let StartTime {
            year,
            month,
            day,
            hour,
            minute,
            second,
        } = self.start;
        let seconds = self.duration_seconds;
        let audio = self.quality.audio_file_name();
        let [summary, decisions, action_items, transcript, audio_heading] = HEADINGS;
        format!(
            "---\n\
             date: {year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}\n\
             duration: {:02}:{:02}:{:02}\n\
             source: {}\n\
             inputs: [{}]\n\
             audio: {audio}\n\
             asr_model: {}\n\
             summary_model: {}\n\
             ---\n\
             \n\
             # {year:04}-{month:02}-{day:02} {hour:02}:{minute:02}\n\
             \n\
             ## {summary}\n\n{}\n\n\
             ## {decisions}\n\n{}\n\n\
             ## {action_items}\n\n{}\n\n\
             ## {transcript}\n\n{}\n\n\
             ## {audio_heading}\n\n[{audio}]({audio})\n",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60,
            self.source.as_str(),
            self.inputs().join(", "),
            yaml_scalar(&self.asr_model),
            yaml_scalar(&self.summary_model),
            self.summary.trim(),
            bullets(&self.decisions),
            bullets(&self.action_items),
            self.transcript.trim(),
        )
    }
}

fn bullets(items: &[String]) -> String {
    if items.is_empty() {
        return "-".into();
    }
    items
        .iter()
        .map(|item| format!("- {}", item.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Model names come from the shipped list and are normally plain. Anything
/// that could change the meaning of the front matter is written quoted.
fn yaml_scalar(value: &str) -> String {
    let plain = !value.is_empty()
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " .-_".contains(c))
        && !value.ends_with(' ');
    if plain {
        value.into()
    } else {
        // A JSON string is also a valid double-quoted YAML scalar.
        serde_json::to_string(value).unwrap()
    }
}

/// Writes `note.md` in a recording folder. The text goes to
/// `.anchovy/note.tmp` first and then replaces `note.md` in one step, so a
/// failure leaves the old note as it was.
pub fn write_note(recording_dir: &Path, text: &str) -> io::Result<()> {
    write_note_with(recording_dir, |file| file.write_all(text.as_bytes()))
}

fn write_note_with(
    recording_dir: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<()> {
    let app_dir = recording_dir.join(APP_DIR);
    fs::create_dir_all(&app_dir)?;
    write_atomically(
        &app_dir.join(NOTE_TMP_FILE),
        &recording_dir.join(NOTE_FILE),
        write,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;

    fn note() -> Note {
        Note {
            start: StartTime {
                year: 2026,
                month: 9,
                day: 26,
                hour: 14,
                minute: 10,
                second: 0,
            },
            duration_seconds: 42 * 60,
            source: Source::Meeting,
            computer_audio: true,
            quality: Quality::High,
            asr_model: "Qwen3-ASR 1.7B".into(),
            summary_model: "Qwen3-4B-Instruct-2507".into(),
            summary: "We planned the launch.".into(),
            decisions: vec!["Ship on Friday.".into(), "Keep the beta open.".into()],
            action_items: vec!["Sam writes the release notes.".into()],
            transcript: "Let's ship on Friday.".into(),
        }
    }

    fn front_matter_value<'a>(text: &'a str, key: &str) -> &'a str {
        let prefix = format!("{key}: ");
        assert!(text.starts_with("---\n"));
        text.lines()
            .skip(1)
            .take_while(|line| *line != "---")
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("no {key} in front matter"))
    }

    #[test]
    fn renders_the_spec_shape() {
        let expected = "\
---
date: 2026-09-26T14:10:00
duration: 00:42:00
source: meeting
inputs: [microphone, computer audio]
audio: audio.wav
asr_model: Qwen3-ASR 1.7B
summary_model: Qwen3-4B-Instruct-2507
---

# 2026-09-26 14:10

## Summary

We planned the launch.

## Decisions

- Ship on Friday.
- Keep the beta open.

## Action items

- Sam writes the release notes.

## Transcript

Let's ship on Friday.

## Audio

[audio.wav](audio.wav)
";
        assert_eq!(note().render(), expected);
    }

    #[test]
    fn audio_field_and_link_use_the_real_file_name() {
        for (quality, file) in [(Quality::High, "audio.wav"), (Quality::Small, "audio.m4a")] {
            let text = Note { quality, ..note() }.render();
            assert_eq!(front_matter_value(&text, "audio"), file);
            let last_line = text.trim_end().lines().last().unwrap();
            assert_eq!(last_line, format!("[{file}]({file})"));
        }
    }

    #[test]
    fn headings_are_english_and_in_order() {
        let text = note().render();
        let headings: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .collect();
        assert_eq!(
            headings,
            [
                "Summary",
                "Decisions",
                "Action items",
                "Transcript",
                "Audio"
            ]
        );
        assert_eq!(headings, HEADINGS);
    }

    #[test]
    fn headings_stay_english_when_the_meeting_is_not() {
        let text = Note {
            summary: "我们讨论了发布计划。".into(),
            transcript: "周五发布。".into(),
            ..note()
        }
        .render();
        assert!(text.contains("## Summary\n\n我们讨论了发布计划。\n"));
        assert!(text.contains("## Transcript\n\n周五发布。\n"));
    }

    #[test]
    fn source_is_manual_or_meeting() {
        assert_eq!(Source::Manual.as_str(), "manual");
        assert_eq!(Source::Meeting.as_str(), "meeting");
        let text = Note {
            source: Source::Manual,
            ..note()
        }
        .render();
        assert_eq!(front_matter_value(&text, "source"), "manual");

        assert_eq!(
            serde_json::from_str::<Source>(r#""meeting""#).unwrap(),
            Source::Meeting
        );
        assert!(serde_json::from_str::<Source>(r#""zoom""#).is_err());
        assert!(serde_json::from_str::<Source>(r#""Manual""#).is_err());
    }

    #[test]
    fn inputs_list_only_the_microphone_without_computer_audio() {
        let with = note();
        assert_eq!(with.inputs(), ["microphone", "computer audio"]);

        let without = Note {
            computer_audio: false,
            ..note()
        };
        assert_eq!(without.inputs(), ["microphone"]);
        assert_eq!(
            front_matter_value(&without.render(), "inputs"),
            "[microphone]"
        );
    }

    #[test]
    fn empty_lists_render_an_empty_bullet() {
        let text = Note {
            decisions: vec![],
            action_items: vec![],
            ..note()
        }
        .render();
        assert!(text.contains("## Decisions\n\n-\n\n## Action items\n\n-\n\n## Transcript"));
    }

    #[test]
    fn long_recordings_show_hours_minutes_and_seconds() {
        let text = Note {
            duration_seconds: 3 * 3600 + 5 * 60 + 9,
            ..note()
        }
        .render();
        assert_eq!(front_matter_value(&text, "duration"), "03:05:09");
    }

    #[test]
    fn model_names_that_would_break_front_matter_are_quoted() {
        let text = Note {
            asr_model: "ASR: v2 # beta".into(),
            ..note()
        }
        .render();
        assert_eq!(
            front_matter_value(&text, "asr_model"),
            r#""ASR: v2 # beta""#
        );
    }

    #[test]
    fn write_note_replaces_note_md_and_leaves_no_temp_file() {
        let dir = TestDir::new();
        fs::write(dir.path().join(NOTE_FILE), "old note").unwrap();

        write_note(dir.path(), "new note").unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(NOTE_FILE)).unwrap(),
            "new note"
        );
        assert!(!dir.path().join(APP_DIR).join(NOTE_TMP_FILE).exists());
    }

    #[test]
    fn write_note_goes_through_the_hidden_temp_file() {
        let dir = TestDir::new();
        fs::write(dir.path().join(NOTE_FILE), "old note").unwrap();
        let tmp = dir.path().join(APP_DIR).join(NOTE_TMP_FILE);

        write_note_with(dir.path(), |file| {
            assert!(tmp.exists());
            assert_eq!(
                fs::read_to_string(dir.path().join(NOTE_FILE)).unwrap(),
                "old note"
            );
            file.write_all(b"new note")
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(dir.path().join(NOTE_FILE)).unwrap(),
            "new note"
        );
    }

    #[test]
    fn failed_write_keeps_the_old_note() {
        let dir = TestDir::new();
        fs::write(dir.path().join(NOTE_FILE), "old note").unwrap();

        let result = write_note_with(dir.path(), |file| {
            file.write_all(b"half a no")?;
            Err(io::Error::other("disk full"))
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join(NOTE_FILE)).unwrap(),
            "old note"
        );
        assert!(!dir.path().join(APP_DIR).join(NOTE_TMP_FILE).exists());
    }

    #[test]
    fn failed_first_write_leaves_no_note() {
        let dir = TestDir::new();

        let result = write_note_with(dir.path(), |file| {
            file.write_all(b"half a no")?;
            Err(io::Error::other("disk full"))
        });

        assert!(result.is_err());
        assert!(!dir.path().join(NOTE_FILE).exists());
        assert!(!dir.path().join(APP_DIR).join(NOTE_TMP_FILE).exists());
    }

    #[test]
    fn write_note_never_touches_the_audio() {
        let dir = TestDir::new();
        fs::write(dir.path().join("audio.wav"), b"RIFF").unwrap();

        let _ = write_note_with(dir.path(), |_| Err(io::Error::other("disk full")));
        write_note(dir.path(), "note").unwrap();

        assert_eq!(fs::read(dir.path().join("audio.wav")).unwrap(), b"RIFF");
    }
}
