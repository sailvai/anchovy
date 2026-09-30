//! Small quality: `audio.m4a`, AAC, mono, 48 kHz, from the macOS encoder.
//!
//! A recording must survive the app being killed, and an M4A cannot be
//! played until it is finished. So while recording, the same WAV as High is
//! written to `.anchovy/recording.wav`, where Obsidian does not show it. On
//! stop it is encoded to `audio.m4a`, the M4A is read back and its length
//! checked, and only then is the WAV deleted. A WAV left there by an app that
//! was killed is encoded the same way at the next launch. If encoding fails,
//! the WAV becomes `audio.wav` and `state.json` says why. Nothing here ever
//! deletes audio that has not been saved another way.
//!
//! The encoder is behind [`Encoder`] so this logic is tested without the
//! system; `crate::m4a::mac` is the real one.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};

use super::file_writer::repair_header;
use crate::notes::folder::Quality;
use crate::notes::state::{read_state, write_state, Status};
use crate::notes::{NotesError, APP_DIR};

/// The WAV written while a Small recording runs, inside `.anchovy/`.
pub const PENDING_WAV: &str = "recording.wav";
/// The M4A while it is being encoded, inside `.anchovy/`.
const M4A_TMP: &str = "audio.m4a.tmp";
/// The M4A must read back within this of the WAV's length.
pub const DURATION_TOLERANCE_SECONDS: f64 = 0.05;

/// Encodes and measures M4A files.
pub trait Encoder: Send + Sync {
    /// Encodes the WAV at `wav` into an AAC, mono, 48 kHz M4A at `m4a`.
    fn encode(&self, wav: &Path, m4a: &Path) -> Result<(), String>;
    /// Seconds of audio in `audio`, as the system decoder reads them.
    fn duration(&self, audio: &Path) -> Result<f64, String>;
}

/// Where a Small recording's WAV is written while it runs.
pub fn pending_wav(folder: &Path) -> PathBuf {
    folder.join(APP_DIR).join(PENDING_WAV)
}

/// Where a recording of `quality` writes its audio while it runs.
pub fn recording_path(folder: &Path, quality: Quality) -> PathBuf {
    match quality {
        Quality::High => folder.join(Quality::High.audio_file_name()),
        Quality::Small => pending_wav(folder),
    }
}

/// Where a Small recording's audio ended up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// `audio.m4a`, or `audio.wav` if encoding failed.
    pub audio: PathBuf,
    /// Why the audio is not an M4A, for `state.json`.
    pub failure: Option<String>,
}

/// Turns the pending WAV into `audio.m4a`, or, if that fails, into
/// `audio.wav`. Errs only if even the WAV could not be moved; it then stays
/// where it is, and the next launch tries again.
pub fn finish(folder: &Path, encoder: &dyn Encoder) -> io::Result<Finished> {
    let wav = pending_wav(folder);
    let seconds = repair_header(&wav)?;
    let tmp = folder.join(APP_DIR).join(M4A_TMP);
    let m4a = folder.join(Quality::Small.audio_file_name());
    let encoded = encode_checked(encoder, &wav, &tmp, seconds)
        .and_then(|()| fs::rename(&tmp, &m4a).map_err(|err| sentence(&err.to_string())));
    match encoded {
        Ok(()) => {
            // If this fails, the next launch encodes the WAV again and
            // replaces audio.m4a with the same audio.
            let _ = fs::remove_file(&wav);
            Ok(Finished {
                audio: m4a,
                failure: None,
            })
        }
        Err(reason) => {
            let _ = fs::remove_file(&tmp);
            let kept = folder.join(Quality::High.audio_file_name());
            fs::rename(&wav, &kept)?;
            Ok(Finished {
                audio: kept,
                failure: Some(format!(
                    "Anchovy couldn't save this recording as M4A, so it kept it as WAV. {reason}"
                )),
            })
        }
    }
}

/// Encodes to `tmp`, flushes it to disk, and reads it back: it must hold as
/// much audio as the WAV.
fn encode_checked(
    encoder: &dyn Encoder,
    wav: &Path,
    tmp: &Path,
    seconds: f64,
) -> Result<(), String> {
    encoder.encode(wav, tmp).map_err(|err| sentence(&err))?;
    File::open(tmp)
        .and_then(|file| file.sync_all())
        .map_err(|err| sentence(&err.to_string()))?;
    let read_back = encoder.duration(tmp).map_err(|err| sentence(&err))?;
    if (read_back - seconds).abs() > DURATION_TOLERANCE_SECONDS {
        return Err(format!(
            "The M4A read back as {read_back:.2} s of audio instead of {seconds:.2} s."
        ));
    }
    Ok(())
}

/// System errors often end without a period; `state.json` shows sentences.
fn sentence(text: &str) -> String {
    let text = text.trim();
    if text.ends_with('.') {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

/// At launch: encodes a WAV left by a Small recording whose app was killed,
/// and moves that recording from Recording to Saved. `None` when there is
/// nothing to do.
pub fn recover(folder: &Path, encoder: &dyn Encoder) -> Result<Option<Finished>, NotesError> {
    if !pending_wav(folder).is_file() {
        return Ok(None);
    }
    // The audio first; then what state.json says about it.
    let finished = finish(folder, encoder)?;
    let mut state = read_state(folder)?;
    if state.status == Status::Recording {
        state.move_to(Status::Saved)?;
    }
    state.encoding_failed = finished.failure.clone();
    write_state(folder, &state)?;
    Ok(Some(finished))
}

#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use crate::recording::file_writer::HEADER_BYTES;
    use std::sync::Mutex;

    /// "Encodes" by copying the WAV. Reads a length from the copy's header,
    /// as the system decoder would, so a header that was never finished
    /// reads short.
    #[derive(Default)]
    pub struct CopyEncoder {
        pub fail: Option<String>,
        /// Added to every length it reads back.
        pub skew_seconds: f64,
        pub encoded: Mutex<Vec<PathBuf>>,
    }

    impl Encoder for CopyEncoder {
        fn encode(&self, wav: &Path, m4a: &Path) -> Result<(), String> {
            self.encoded.lock().unwrap().push(wav.to_path_buf());
            if let Some(reason) = &self.fail {
                return Err(reason.clone());
            }
            fs::copy(wav, m4a)
                .map(|_| ())
                .map_err(|err| err.to_string())
        }

        fn duration(&self, audio: &Path) -> Result<f64, String> {
            let bytes = fs::read(audio).map_err(|err| err.to_string())?;
            let header = bytes.get(40..HEADER_BYTES as usize).ok_or("not audio")?;
            let data = u32::from_le_bytes(header.try_into().unwrap());
            Ok(f64::from(data) / 96_000.0 + self.skew_seconds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::CopyEncoder;
    use super::*;
    use crate::library::Library;
    use crate::m4a::mac::MacEncoder;
    use crate::notes::folder::{create_recording_folder, StartTime};
    use crate::notes::state::State;
    use crate::notes::test_dir::TestDir;
    use crate::recording::file_writer::WavWriter;
    use crate::recording::mixer::OUTPUT_RATE;
    use std::io::BufWriter;

    const START: StartTime = StartTime {
        year: 2026,
        month: 9,
        day: 29,
        hour: 10,
        minute: 0,
        second: 0,
    };

    /// A Small recording's folder with `seconds` of a tone in its pending
    /// WAV. `finished`: whether the WAV header was closed, as on stop, or
    /// left as a killed app leaves it.
    fn small_recording(dir: &TestDir, seconds: f64, finished: bool) -> PathBuf {
        let folder = create_recording_folder(dir.path(), &START).unwrap();
        write_state(&folder, &State::new()).unwrap();
        let samples: Vec<f32> = (0..(seconds * f64::from(OUTPUT_RATE)) as usize)
            .map(|i| 0.3 * (i as f32 * 0.06).sin())
            .collect();
        let file = BufWriter::new(File::create(pending_wav(&folder)).unwrap());
        let mut writer = WavWriter::new(file, OUTPUT_RATE).unwrap();
        writer.write(&samples).unwrap();
        if finished {
            writer.finish().unwrap();
        } else {
            // Killed before the first header update: the samples are on
            // disk, and the header still says 0 bytes.
            drop(writer);
        }
        folder
    }

    fn names(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn stopping_encodes_the_pending_wav_to_audio_m4a_then_deletes_it() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 2.0, true);
        let wav = fs::read(pending_wav(&folder)).unwrap();
        let encoder = CopyEncoder::default();

        let finished = finish(&folder, &encoder).unwrap();

        assert_eq!(
            finished,
            Finished {
                audio: folder.join("audio.m4a"),
                failure: None,
            }
        );
        assert_eq!(fs::read(folder.join("audio.m4a")).unwrap(), wav);
        assert!(!pending_wav(&folder).exists());
        assert!(!folder.join(APP_DIR).join(M4A_TMP).exists());
        assert_eq!(names(&folder), [".anchovy", "audio.m4a"]);
    }

    #[test]
    fn a_failed_encode_keeps_the_recording_as_audio_wav_with_the_reason() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 2.0, true);
        let wav = fs::read(pending_wav(&folder)).unwrap();
        let encoder = CopyEncoder {
            fail: Some("The AAC encoder is not available.".into()),
            ..CopyEncoder::default()
        };

        let finished = finish(&folder, &encoder).unwrap();

        assert_eq!(finished.audio, folder.join("audio.wav"));
        assert_eq!(
            finished.failure.as_deref(),
            Some(
                "Anchovy couldn't save this recording as M4A, so it kept it as WAV. The AAC \
                 encoder is not available."
            )
        );
        assert_eq!(fs::read(folder.join("audio.wav")).unwrap(), wav);
        assert_eq!(names(&folder), [".anchovy", "audio.wav"]);
        assert!(!folder.join(APP_DIR).join(M4A_TMP).exists());
    }

    #[test]
    fn an_m4a_that_reads_back_the_wrong_length_is_not_kept() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 2.0, true);
        let encoder = CopyEncoder {
            skew_seconds: -0.2,
            ..CopyEncoder::default()
        };

        let finished = finish(&folder, &encoder).unwrap();

        assert_eq!(finished.audio, folder.join("audio.wav"));
        assert_eq!(
            finished.failure.as_deref(),
            Some(
                "Anchovy couldn't save this recording as M4A, so it kept it as WAV. The M4A \
                 read back as 1.80 s of audio instead of 2.00 s."
            )
        );
        assert_eq!(names(&folder), [".anchovy", "audio.wav"]);
    }

    #[test]
    fn a_small_difference_in_length_is_within_the_check() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 2.0, true);
        let encoder = CopyEncoder {
            skew_seconds: 0.04,
            ..CopyEncoder::default()
        };

        assert_eq!(finish(&folder, &encoder).unwrap().failure, None);
    }

    #[test]
    fn a_leftover_pending_wav_is_encoded_at_launch_and_the_recording_saved() {
        let dir = TestDir::new();
        // The app was killed: the WAV header was never updated.
        let folder = small_recording(&dir, 1.5, false);
        let mut state = read_state(&folder).unwrap();
        state.source = crate::notes::note::Source::Meeting;
        write_state(&folder, &state).unwrap();
        let encoder = CopyEncoder::default();

        let finished = recover(&folder, &encoder).unwrap();

        assert_eq!(
            finished,
            Some(Finished {
                audio: folder.join("audio.m4a"),
                failure: None,
            })
        );
        assert_eq!(names(&folder), [".anchovy", "audio.m4a"]);
        let state = read_state(&folder).unwrap();
        assert_eq!(state.status, Status::Saved);
        assert_eq!(state.encoding_failed, None);
        assert_eq!(state.source, crate::notes::note::Source::Meeting);
        // Every sample on disk made it, not just what the header said.
        assert!((encoder.duration(&folder.join("audio.m4a")).unwrap() - 1.5).abs() < 1e-9);
    }

    #[test]
    fn a_failed_encode_at_launch_keeps_audio_wav_and_records_the_reason() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 1.0, false);
        let encoder = CopyEncoder {
            fail: Some("Disk full.".into()),
            ..CopyEncoder::default()
        };

        recover(&folder, &encoder).unwrap();

        assert_eq!(names(&folder), [".anchovy", "audio.wav"]);
        let state = read_state(&folder).unwrap();
        assert_eq!(state.status, Status::Saved);
        assert_eq!(
            state.encoding_failed.as_deref(),
            Some("Anchovy couldn't save this recording as M4A, so it kept it as WAV. Disk full.")
        );
        // The header was repaired, so the WAV plays to the end.
        let bytes = fs::read(folder.join("audio.wav")).unwrap();
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(u64::from(data) + 44, bytes.len() as u64);
    }

    #[test]
    fn a_launch_that_cannot_save_the_state_keeps_the_wav_for_the_next_one() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 1.0, false);
        fs::create_dir(folder.join(APP_DIR).join("state.tmp")).unwrap();
        let encoder = CopyEncoder::default();

        assert!(recover(&folder, &encoder).is_err());

        assert_eq!(read_state(&folder).unwrap().status, Status::Recording);
        assert!(pending_wav(&folder).is_file());
        fs::remove_dir(folder.join(APP_DIR).join("state.tmp")).unwrap();
        recover(&folder, &encoder).unwrap().unwrap();
        assert_eq!(read_state(&folder).unwrap().status, Status::Saved);
        assert_eq!(names(&folder), [".anchovy", "audio.m4a"]);
    }

    #[test]
    fn without_a_pending_wav_launch_changes_nothing() {
        let dir = TestDir::new();
        let folder = create_recording_folder(dir.path(), &START).unwrap();
        fs::write(folder.join("audio.wav"), b"RIFF").unwrap();
        let mut state = State::new();
        state.move_to(Status::Saved).unwrap();
        write_state(&folder, &state).unwrap();
        let encoder = CopyEncoder::default();

        assert_eq!(recover(&folder, &encoder).unwrap(), None);

        assert!(encoder.encoded.lock().unwrap().is_empty());
        assert_eq!(read_state(&folder).unwrap(), state);
    }

    #[test]
    fn a_recording_recovered_once_is_left_alone_after() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 1.0, true);
        let encoder = CopyEncoder::default();
        recover(&folder, &encoder).unwrap();

        assert_eq!(recover(&folder, &encoder).unwrap(), None);
        assert_eq!(read_state(&folder).unwrap().status, Status::Saved);
    }

    /// The real macOS encoder, on this Mac.
    #[test]
    fn the_system_encoder_writes_an_aac_m4a_that_reads_back_with_the_right_length() {
        let dir = TestDir::new();
        let folder = small_recording(&dir, 3.3, true);

        let finished = finish(&folder, &MacEncoder).unwrap();

        assert_eq!(finished.failure, None, "{finished:?}");
        let m4a = folder.join("audio.m4a");
        assert_eq!(finished.audio, m4a);
        let seconds = MacEncoder.duration(&m4a).unwrap();
        assert!((seconds - 3.3).abs() <= 0.05, "{seconds} s");
        let format = crate::m4a::mac::describe(&m4a).unwrap();
        assert_eq!(format.format_id, u32::from_be_bytes(*b"aac "));
        assert_eq!(format.sample_rate, 48_000.0);
        assert_eq!(format.channels, 1);
        assert!(!pending_wav(&folder).exists());
        // The library lists it with its length.
        let library = Library::new(dir.path().to_path_buf());
        assert_eq!(library.list().unwrap()[0].duration_seconds, Some(3));
    }
}
