//! The recording library: scans the notes folder for recording folders,
//! reports each one's start time, duration, and status, reads `note.md` for
//! display, and moves recordings to the Trash.
//!
//! The notes folder may be an Obsidian vault, so only folders named like a
//! recording (`yyyy-MM-dd-HHmm`, maybe with `-2`) that hold app state, a note,
//! or audio count as recordings. Everything else is left alone.

pub mod commands;
pub mod mac;

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::Serialize;

use crate::notes::folder::audio_file;
use crate::notes::note::NOTE_FILE;
use crate::notes::state::{read_state, Status, STATE_FILE};
use crate::notes::APP_DIR;

/// Shown when `state.json` exists but cannot be read.
pub const UNREADABLE_STATE: &str = "Anchovy can't read this recording's state.";

/// One row in the recording list.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recording {
    /// Folder name inside the notes folder. Also the recording's id.
    pub folder: String,
    /// Local start time, `yyyy-MM-ddTHH:mm`, from the folder name.
    pub start: String,
    /// From the audio file's header. `None` if there is no readable audio.
    pub duration_seconds: Option<u64>,
    #[serde(flatten)]
    pub status: Status,
}

/// The parts of `note.md` the note view shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteView {
    pub source: Option<String>,
    pub inputs: Vec<String>,
    /// Each `## ` section in order, with its body trimmed.
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Section {
    pub heading: String,
    pub body: String,
}

#[derive(Debug)]
pub enum LibraryError {
    /// No recording folder with this name in the notes folder.
    NotFound(String),
    Io(io::Error),
}

impl fmt::Display for LibraryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LibraryError::NotFound(folder) => write!(f, "No recording named {folder}."),
            LibraryError::Io(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LibraryError {}

impl From<io::Error> for LibraryError {
    fn from(err: io::Error) -> Self {
        LibraryError::Io(err)
    }
}

/// The recordings in the notes folder. Until the user chooses a folder on
/// first launch there is none, and the library is empty.
pub struct Library {
    notes_dir: RwLock<Option<PathBuf>>,
}

impl Library {
    pub fn new(notes_dir: PathBuf) -> Self {
        Library {
            notes_dir: RwLock::new(Some(notes_dir)),
        }
    }

    pub fn without_folder() -> Self {
        Library {
            notes_dir: RwLock::new(None),
        }
    }

    pub fn notes_dir(&self) -> Option<PathBuf> {
        self.notes_dir.read().unwrap().clone()
    }

    /// Scans `notes_dir` from now on.
    pub fn set_notes_dir(&self, notes_dir: PathBuf) {
        *self.notes_dir.write().unwrap() = Some(notes_dir);
    }

    /// Every recording in the notes folder, newest first. A notes folder that
    /// does not exist yet, or has not been chosen, has no recordings.
    pub fn list(&self) -> io::Result<Vec<Recording>> {
        let Some(notes_dir) = self.notes_dir() else {
            return Ok(Vec::new());
        };
        let entries = match fs::read_dir(&notes_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let mut found = Vec::new();
        for entry in entries {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(key) = FolderName::parse(&name) else {
                continue;
            };
            if !entry.file_type()?.is_dir() {
                continue;
            }
            if let Some(recording) = read_recording(&entry.path(), &name, &key) {
                found.push((key, recording));
            }
        }
        found.sort_by(|(a, _), (b, _)| b.cmp(a));
        Ok(found.into_iter().map(|(_, recording)| recording).collect())
    }

    /// The path of a recording folder. Only plain recording folder names are
    /// accepted, so a name can never point outside the notes folder.
    pub fn path_of(&self, folder: &str) -> Result<PathBuf, LibraryError> {
        let not_found = || LibraryError::NotFound(folder.into());
        FolderName::parse(folder).ok_or_else(not_found)?;
        let path = self.notes_dir().ok_or_else(not_found)?.join(folder);
        if !path.is_dir() {
            return Err(not_found());
        }
        Ok(path)
    }

    /// The recording's `audio.wav` or `audio.m4a`, inside the notes folder.
    /// `None` before either exists.
    pub fn audio_of(&self, folder: &str) -> Result<Option<PathBuf>, LibraryError> {
        Ok(audio_file(&self.path_of(folder)?).map(|(path, _)| path))
    }

    pub fn read_note(&self, folder: &str) -> Result<NoteView, LibraryError> {
        let text = fs::read_to_string(self.path_of(folder)?.join(NOTE_FILE))?;
        Ok(parse_note(&text))
    }

    /// Hands the recording folder to `trash`, which moves it to the Trash.
    /// Nothing is deleted here.
    pub fn move_to_trash(
        &self,
        folder: &str,
        trash: impl FnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), LibraryError> {
        trash(&self.path_of(folder)?)?;
        Ok(())
    }
}

/// A parsed recording folder name. Sorts by start time, then by the `-2`,
/// `-3` number given to recordings started in the same minute.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FolderName {
    /// `[year, month, day, hour, minute]`.
    start: [u16; 5],
    number: u32,
}

impl FolderName {
    fn parse(name: &str) -> Option<Self> {
        let (base, number) = match name.get(15..) {
            Some("") => (name, 1),
            Some(rest) => {
                let digits = rest.strip_prefix('-')?;
                if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                (&name[..15], digits.parse().ok().filter(|n| *n >= 2)?)
            }
            None => return None,
        };
        let bytes = base.as_bytes();
        let shape_ok = bytes.iter().enumerate().all(|(i, b)| match i {
            4 | 7 | 10 => *b == b'-',
            _ => b.is_ascii_digit(),
        });
        if !shape_ok {
            return None;
        }
        let num = |range: std::ops::Range<usize>| base[range].parse::<u16>().ok();
        let start = [
            num(0..4)?,
            num(5..7)?,
            num(8..10)?,
            num(11..13)?,
            num(13..15)?,
        ];
        let [_, month, day, hour, minute] = start;
        let valid =
            (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60;
        valid.then_some(FolderName { start, number })
    }

    /// `yyyy-MM-ddTHH:mm`.
    fn start_text(&self) -> String {
        let [year, month, day, hour, minute] = self.start;
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}")
    }
}

/// Reads one recording folder, or `None` if it holds nothing Anchovy wrote.
fn read_recording(dir: &Path, name: &str, key: &FolderName) -> Option<Recording> {
    let audio = audio_file(dir).map(|(path, _)| path);
    let has_state = dir.join(APP_DIR).join(STATE_FILE).is_file();
    let has_note = dir.join(NOTE_FILE).is_file();
    let status = if has_state {
        match read_state(dir) {
            Ok(state) => state.status,
            Err(_) => Status::Failed {
                reason: UNREADABLE_STATE.into(),
            },
        }
    } else if has_note {
        Status::Ready
    } else if audio.is_some() {
        Status::Saved
    } else {
        return None;
    };
    Some(Recording {
        folder: name.into(),
        start: key.start_text(),
        duration_seconds: audio.and_then(|path| audio_duration(&path).ok().flatten()),
        status,
    })
}

/// Seconds of audio, from a WAV or M4A header. `None` if the header does not
/// say.
fn audio_duration(path: &Path) -> io::Result<Option<u64>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("wav") => wav_duration(&mut file, len),
        Some("m4a") => m4a_duration(&mut file, len),
        _ => Ok(None),
    }
}

fn read_array<const N: usize>(file: &mut File) -> io::Result<[u8; N]> {
    let mut buf = [0; N];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Walks the RIFF chunks for the byte rate in `fmt ` and the size of `data`.
fn wav_duration(file: &mut File, len: u64) -> io::Result<Option<u64>> {
    let header: [u8; 12] = read_array(file)?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Ok(None);
    }
    let mut byte_rate = None;
    loop {
        let chunk: [u8; 8] = read_array(file)?;
        let size = u32::from_le_bytes(chunk[4..].try_into().unwrap());
        match &chunk[..4] {
            b"fmt " => {
                if size < 16 {
                    return Ok(None);
                }
                let fmt: [u8; 12] = read_array(file)?;
                byte_rate = Some(u32::from_le_bytes(fmt[8..].try_into().unwrap()) as u64);
                file.seek(SeekFrom::Current(size as i64 - 12 + (size & 1) as i64))?;
            }
            b"data" => {
                let Some(rate) = byte_rate.filter(|rate| *rate > 0) else {
                    return Ok(None);
                };
                let start = file.stream_position()?;
                let on_disk = len.saturating_sub(start);
                // While recording, the size is 0 or a placeholder until the
                // file is closed. The bytes on disk are the truth then.
                let size = match size as u64 {
                    0 | 0xFFFF_FFFF => on_disk,
                    size => size.min(on_disk),
                };
                return Ok(Some(size / rate));
            }
            _ => {
                file.seek(SeekFrom::Current(size as i64 + (size & 1) as i64))?;
            }
        }
    }
}

/// Finds `moov/mvhd` and divides its duration by its timescale.
fn m4a_duration(file: &mut File, len: u64) -> io::Result<Option<u64>> {
    let Some((moov_start, moov_end)) = find_box(file, 0, len, b"moov")? else {
        return Ok(None);
    };
    let Some((mvhd, _)) = find_box(file, moov_start, moov_end, b"mvhd")? else {
        return Ok(None);
    };
    file.seek(SeekFrom::Start(mvhd))?;
    let [version, ..]: [u8; 4] = read_array(file)?;
    let (timescale, duration) = if version == 1 {
        let _times: [u8; 16] = read_array(file)?;
        let timescale = u32::from_be_bytes(read_array(file)?) as u64;
        (timescale, u64::from_be_bytes(read_array(file)?))
    } else {
        let _times: [u8; 8] = read_array(file)?;
        let timescale = u32::from_be_bytes(read_array(file)?) as u64;
        (timescale, u32::from_be_bytes(read_array(file)?) as u64)
    };
    Ok((timescale > 0).then(|| duration / timescale))
}

/// Looks for a box of `kind` between `start` and `end` and returns where its
/// body starts and ends.
fn find_box(
    file: &mut File,
    start: u64,
    end: u64,
    kind: &[u8; 4],
) -> io::Result<Option<(u64, u64)>> {
    let mut at = start;
    while at + 8 <= end {
        file.seek(SeekFrom::Start(at))?;
        let header: [u8; 8] = read_array(file)?;
        let mut size = u32::from_be_bytes(header[..4].try_into().unwrap()) as u64;
        let mut body = at + 8;
        if size == 1 {
            size = u64::from_be_bytes(read_array(file)?);
            body += 8;
        } else if size == 0 {
            size = end - at;
        }
        if size < body - at {
            return Ok(None);
        }
        let box_end = at.saturating_add(size).min(end);
        if &header[4..] == kind {
            return Ok(Some((body, box_end)));
        }
        at = box_end;
    }
    Ok(None)
}

/// Splits `note.md` into front matter values and `## ` sections. The user may
/// have edited the note, so anything missing is simply left out.
fn parse_note(text: &str) -> NoteView {
    let mut source = None;
    let mut inputs = Vec::new();
    let mut body = text;
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---\n") {
            for line in rest[..end].lines() {
                let Some((key, value)) = line.split_once(": ") else {
                    continue;
                };
                match key {
                    "source" => source = Some(value.trim().to_owned()),
                    "inputs" => {
                        inputs = value
                            .trim()
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .split(',')
                            .map(|input| input.trim().to_owned())
                            .filter(|input| !input.is_empty())
                            .collect()
                    }
                    _ => {}
                }
            }
            body = &rest[end + 5..];
        }
    }
    let mut sections: Vec<Section> = Vec::new();
    for line in body.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            sections.push(Section {
                heading: heading.trim().to_owned(),
                body: String::new(),
            });
        } else if let Some(section) = sections.last_mut() {
            section.body.push_str(line);
            section.body.push('\n');
        }
    }
    for section in &mut sections {
        section.body = section.body.trim().to_owned();
    }
    NoteView {
        source,
        inputs,
        sections,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::note::Source;
    use crate::notes::state::{write_state, State};
    use crate::notes::test_dir::TestDir;

    /// A 16-bit mono WAV header at `rate` Hz for `seconds` of audio. The
    /// samples themselves are zeros.
    fn wav(rate: u32, seconds: u32) -> Vec<u8> {
        let byte_rate = rate * 2;
        let data_len = byte_rate * seconds;
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data_len).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes()); // PCM
        out.extend(1u16.to_le_bytes()); // mono
        out.extend(rate.to_le_bytes());
        out.extend(byte_rate.to_le_bytes());
        out.extend(2u16.to_le_bytes()); // block align
        out.extend(16u16.to_le_bytes()); // bits
        out.extend(b"data");
        out.extend(data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend(kind);
        out.extend(body);
        out
    }

    /// An M4A with an `ftyp`, an `mdat`, and a `moov` holding a version 0
    /// `mvhd`.
    fn m4a(timescale: u32, duration: u32) -> Vec<u8> {
        let mut mvhd = vec![0u8; 4]; // version 0, flags
        mvhd.extend(0u32.to_be_bytes()); // creation
        mvhd.extend(0u32.to_be_bytes()); // modification
        mvhd.extend(timescale.to_be_bytes());
        mvhd.extend(duration.to_be_bytes());
        mvhd.resize(100, 0);
        let mut out = mp4_box(b"ftyp", b"M4A \0\0\0\0");
        out.extend(mp4_box(b"mdat", &[0; 64]));
        out.extend(mp4_box(b"moov", &mp4_box(b"mvhd", &mvhd)));
        out
    }

    fn status(status: Status) -> State {
        State {
            status,
            asr_model: None,
            summary_model: None,
            inputs: Vec::new(),
            source: Source::Manual,
            encoding_failed: None,
        }
    }

    /// Creates a recording folder with a one-second-per-minute WAV so the
    /// duration shows which folder is which.
    fn recording(notes: &Path, folder: &str, minutes: u32, state: Option<Status>) -> PathBuf {
        let dir = notes.join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("audio.wav"), wav(100, minutes * 60)).unwrap();
        if let Some(state) = state {
            write_state(&dir, &status(state)).unwrap();
        }
        dir
    }

    fn folders(list: &[Recording]) -> Vec<&str> {
        list.iter().map(|item| item.folder.as_str()).collect()
    }

    #[test]
    fn lists_recordings_newest_first_with_start_duration_and_status() {
        let notes = TestDir::new();
        recording(notes.path(), "2026-09-25-1645", 27, Some(Status::Ready));
        recording(notes.path(), "2026-09-26-1410", 42, Some(Status::Working));
        recording(notes.path(), "2026-09-26-0905", 66, Some(failed()));
        recording(
            notes.path(),
            "2026-09-26-1130",
            18,
            Some(Status::NeedsModels),
        );
        recording(notes.path(), "2026-09-25-1000", 9, Some(Status::Saved));

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(
            folders(&list),
            [
                "2026-09-26-1410",
                "2026-09-26-1130",
                "2026-09-26-0905",
                "2026-09-25-1645",
                "2026-09-25-1000",
            ]
        );
        assert_eq!(
            list[0],
            Recording {
                folder: "2026-09-26-1410".into(),
                start: "2026-09-26T14:10".into(),
                duration_seconds: Some(42 * 60),
                status: Status::Working,
            }
        );
        let statuses: Vec<&Status> = list.iter().map(|item| &item.status).collect();
        assert_eq!(
            statuses,
            [
                &Status::Working,
                &Status::NeedsModels,
                &failed(),
                &Status::Ready,
                &Status::Saved,
            ]
        );
    }

    fn failed() -> Status {
        Status::Failed {
            reason: "Not enough memory".into(),
        }
    }

    #[test]
    fn same_minute_recordings_sort_by_their_number() {
        let notes = TestDir::new();
        for folder in [
            "2026-09-26-1410",
            "2026-09-26-1410-2",
            "2026-09-26-1410-10",
            "2026-09-26-1409",
        ] {
            recording(notes.path(), folder, 1, Some(Status::Saved));
        }

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(
            folders(&list),
            [
                "2026-09-26-1410-10",
                "2026-09-26-1410-2",
                "2026-09-26-1410",
                "2026-09-26-1409",
            ]
        );
        assert!(list[..3]
            .iter()
            .all(|item| item.start == "2026-09-26T14:10"));
    }

    #[test]
    fn status_serializes_the_way_the_interface_reads_it() {
        let item = Recording {
            folder: "2026-09-26-0905".into(),
            start: "2026-09-26T09:05".into(),
            duration_seconds: None,
            status: failed(),
        };
        assert_eq!(
            serde_json::to_value(item).unwrap(),
            serde_json::json!({
                "folder": "2026-09-26-0905",
                "start": "2026-09-26T09:05",
                "duration_seconds": null,
                "status": "failed",
                "reason": "Not enough memory",
            })
        );
    }

    #[test]
    fn folders_without_state_fall_back_to_what_is_on_disk() {
        let notes = TestDir::new();
        // Audio only: saved, no note yet.
        recording(notes.path(), "2026-09-26-1000", 3, None);
        // A note next to the audio: ready.
        let ready = recording(notes.path(), "2026-09-26-1100", 4, None);
        fs::write(ready.join(NOTE_FILE), "# note").unwrap();

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(list[0].status, Status::Ready);
        assert_eq!(list[1].status, Status::Saved);
    }

    #[test]
    fn unreadable_state_shows_as_failed_instead_of_hiding_the_recording() {
        let notes = TestDir::new();
        let dir = recording(notes.path(), "2026-09-26-1000", 3, None);
        fs::create_dir_all(dir.join(APP_DIR)).unwrap();
        fs::write(dir.join(APP_DIR).join(STATE_FILE), "{not json").unwrap();

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(
            list[0].status,
            Status::Failed {
                reason: UNREADABLE_STATE.into()
            }
        );
    }

    #[test]
    fn ignores_everything_that_is_not_a_recording_folder() {
        let notes = TestDir::new();
        recording(notes.path(), "2026-09-26-1410", 1, Some(Status::Saved));
        // An Obsidian vault's own folders and files.
        fs::create_dir_all(notes.path().join(".obsidian")).unwrap();
        fs::create_dir_all(notes.path().join("Projects")).unwrap();
        fs::write(notes.path().join("Projects").join("audio.wav"), wav(100, 1)).unwrap();
        fs::write(notes.path().join("Inbox.md"), "hello").unwrap();
        // Named like a recording but empty, or a file, or out of range.
        fs::create_dir_all(notes.path().join("2026-09-26-1500")).unwrap();
        fs::write(notes.path().join("2026-09-26-1600"), "").unwrap();
        recording(notes.path(), "2026-13-26-1410", 1, Some(Status::Saved));
        recording(notes.path(), "2026-09-26-2460", 1, Some(Status::Saved));
        recording(notes.path(), "2026-09-26-1410-x", 1, Some(Status::Saved));

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(folders(&list), ["2026-09-26-1410"]);
    }

    #[test]
    fn a_missing_notes_folder_has_no_recordings() {
        let notes = TestDir::new();
        let library = Library::new(notes.path().join("Anchovy"));
        assert_eq!(library.list().unwrap(), []);
    }

    #[test]
    fn duration_comes_from_a_wav_or_m4a_header() {
        let notes = TestDir::new();
        let wav_dir = notes.path().join("2026-09-26-1000");
        fs::create_dir_all(&wav_dir).unwrap();
        fs::write(wav_dir.join("audio.wav"), wav(48_000, 3)).unwrap();
        let m4a_dir = notes.path().join("2026-09-26-1100");
        fs::create_dir_all(&m4a_dir).unwrap();
        fs::write(m4a_dir.join("audio.m4a"), m4a(44_100, 44_100 * 125)).unwrap();

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(list[0].duration_seconds, Some(125));
        assert_eq!(list[1].duration_seconds, Some(3));
    }

    #[test]
    fn a_wav_still_being_written_is_measured_by_its_size() {
        // While recording, the data chunk size is not filled in yet.
        let mut bytes = wav(100, 7);
        bytes[40..44].copy_from_slice(&0u32.to_le_bytes());
        let notes = TestDir::new();
        let dir = notes.path().join("2026-09-26-1000");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("audio.wav"), bytes).unwrap();
        write_state(&dir, &status(Status::Recording)).unwrap();

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(list[0].status, Status::Recording);
        assert_eq!(list[0].duration_seconds, Some(7));
    }

    #[test]
    fn unreadable_audio_has_no_duration() {
        let notes = TestDir::new();
        let dir = notes.path().join("2026-09-26-1000");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("audio.wav"), b"RIFF").unwrap();
        let empty = notes.path().join("2026-09-26-1100");
        fs::create_dir_all(&empty).unwrap();
        write_state(&empty, &status(Status::Saved)).unwrap();

        let list = Library::new(notes.path().into()).list().unwrap();

        assert_eq!(list[0].duration_seconds, None);
        assert_eq!(list[1].duration_seconds, None);
    }

    #[test]
    fn move_to_trash_hands_over_the_folder_and_the_list_updates() {
        let notes = TestDir::new();
        let trash = TestDir::new();
        recording(notes.path(), "2026-09-26-1410", 1, Some(Status::Ready));
        let gone = recording(notes.path(), "2026-09-26-1130", 1, Some(Status::Saved));
        let library = Library::new(notes.path().into());
        assert_eq!(library.list().unwrap().len(), 2);

        let mut handed = None;
        library
            .move_to_trash("2026-09-26-1130", |path| {
                handed = Some(path.to_path_buf());
                fs::rename(path, trash.path().join("2026-09-26-1130"))
            })
            .unwrap();

        assert_eq!(handed, Some(gone));
        assert_eq!(folders(&library.list().unwrap()), ["2026-09-26-1410"]);
        // Moved, not deleted: the audio is in the trash.
        assert!(trash
            .path()
            .join("2026-09-26-1130")
            .join("audio.wav")
            .exists());
    }

    #[test]
    fn move_to_trash_failure_keeps_the_recording() {
        let notes = TestDir::new();
        recording(notes.path(), "2026-09-26-1130", 1, Some(Status::Saved));
        let library = Library::new(notes.path().into());

        let result = library.move_to_trash("2026-09-26-1130", |_| {
            Err(io::Error::other("The Trash is not available."))
        });

        assert!(matches!(result, Err(LibraryError::Io(_))));
        assert_eq!(folders(&library.list().unwrap()), ["2026-09-26-1130"]);
    }

    #[test]
    fn names_outside_the_library_are_rejected() {
        let notes = TestDir::new();
        recording(notes.path(), "2026-09-26-1130", 1, Some(Status::Saved));
        fs::create_dir_all(notes.path().join("Projects")).unwrap();
        let library = Library::new(notes.path().join("inner"));
        fs::create_dir_all(notes.path().join("inner")).unwrap();

        for name in [
            "../2026-09-26-1130",
            "..",
            "",
            "/tmp",
            "Projects",
            "2026-09-26-1130/..",
            "2026-09-26-1200",
        ] {
            let mut called = false;
            let result = library.move_to_trash(name, |_| {
                called = true;
                Ok(())
            });
            assert!(matches!(result, Err(LibraryError::NotFound(_))), "{name:?}");
            assert!(!called, "{name:?}");
            assert!(matches!(
                library.read_note(name),
                Err(LibraryError::NotFound(_))
            ));
        }
        assert!(notes.path().join("2026-09-26-1130").exists());
    }

    #[test]
    fn path_of_returns_the_folder_inside_the_notes_folder() {
        let notes = TestDir::new();
        let dir = recording(notes.path(), "2026-09-26-1130", 1, Some(Status::Saved));
        let library = Library::new(notes.path().into());
        assert_eq!(library.path_of("2026-09-26-1130").unwrap(), dir);
    }

    const NOTE: &str = "\
---
date: 2026-09-25T16:45:00
duration: 00:27:14
source: manual
inputs: [microphone, computer audio]
audio: audio.wav
asr_model: Qwen3-ASR 1.7B
summary_model: Qwen3-4B-Instruct-2507
---

# 2026-09-25 16:45

## Summary

The team reviewed the October release.

## Decisions

- Ship on Wednesday.
- Keep the old export format.

## Action items

- Send the checklist.

## Transcript

00:00:04 Okay, let's start.
00:00:11 It's merged.

## Audio

[audio.wav](audio.wav)
";

    #[test]
    fn reads_the_note_front_matter_and_sections() {
        let notes = TestDir::new();
        let dir = recording(notes.path(), "2026-09-25-1645", 27, Some(Status::Ready));
        fs::write(dir.join(NOTE_FILE), NOTE).unwrap();

        let note = Library::new(notes.path().into())
            .read_note("2026-09-25-1645")
            .unwrap();

        assert_eq!(note.source.as_deref(), Some("manual"));
        assert_eq!(note.inputs, ["microphone", "computer audio"]);
        let headings: Vec<&str> = note.sections.iter().map(|s| s.heading.as_str()).collect();
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
        assert_eq!(
            note.sections[1].body,
            "- Ship on Wednesday.\n- Keep the old export format."
        );
        assert_eq!(
            note.sections[3].body,
            "00:00:04 Okay, let's start.\n00:00:11 It's merged."
        );
    }

    #[test]
    fn reads_a_note_the_user_edited_without_front_matter() {
        let notes = TestDir::new();
        let dir = recording(notes.path(), "2026-09-25-1645", 27, Some(Status::Ready));
        fs::write(
            dir.join(NOTE_FILE),
            "Intro line\n\n## Summary\n\n我们讨论了发布计划。\n\n## My notes\n\nExtra\n",
        )
        .unwrap();

        let note = Library::new(notes.path().into())
            .read_note("2026-09-25-1645")
            .unwrap();

        assert_eq!(note.source, None);
        assert_eq!(note.inputs, Vec::<String>::new());
        assert_eq!(
            note.sections,
            [
                Section {
                    heading: "Summary".into(),
                    body: "我们讨论了发布计划。".into()
                },
                Section {
                    heading: "My notes".into(),
                    body: "Extra".into()
                },
            ]
        );
    }

    #[test]
    fn a_missing_note_is_an_io_error() {
        let notes = TestDir::new();
        recording(notes.path(), "2026-09-25-1645", 27, Some(Status::Saved));
        let result = Library::new(notes.path().into()).read_note("2026-09-25-1645");
        assert!(matches!(result, Err(LibraryError::Io(_))));
    }
}
