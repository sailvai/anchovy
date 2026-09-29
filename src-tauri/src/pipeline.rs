//! From a saved recording to `note.md`.
//!
//! 1. Read the audio and convert it to 16 kHz mono.
//! 2. Transcribe it in overlapping windows and join the text.
//! 3. Unload the transcription model and check that its memory came back.
//! 4. Load the summary model and ask for the summary, decisions, and action
//!    items as JSON, in chunks if the transcript is long.
//! 5. Write `note.md` atomically, then mark the recording Ready.
//!
//! Any failure marks the recording Failed with a reason whose first sentence
//! is short enough for the recording list. The audio is never changed, and
//! `note.md` is only ever replaced whole.
//!
//! One note is written at a time, on a worker thread. Everything here runs
//! against the `Transcriber` and `Summarizer` traits, so the tests use fakes.

pub mod commands;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::engines::audio::{read_model_audio, MODEL_RATE};
use crate::engines::summary::{self, Summary};
use crate::engines::windows::{self, Joiner, Segment, OVERLAP_SECONDS, WINDOW_SECONDS};
use crate::engines::{Engines, ModelFiles};
use crate::models::catalog::Role;
use crate::notes::folder::{Quality, StartTime};
use crate::notes::note::{write_note, Note, Source, NOTE_FILE};
use crate::notes::state::{read_state, write_state, Input, State, Status};
use crate::notes::NotesError;

/// The selected models, as the pipeline needs them.
pub trait ModelSource: Send + Sync {
    /// The model selected for `role`, and whether it is on this Mac and
    /// passed its checksums.
    fn selected(&self, role: Role) -> (ModelFiles, bool);
}

impl ModelSource for crate::models::Models {
    fn selected(&self, role: Role) -> (ModelFiles, bool) {
        self.selected_files(role)
    }
}

/// Memory readings, from `engines::mac` in the app.
pub trait Memory: Send + Sync {
    /// This process's physical footprint, including GPU memory.
    fn footprint(&self) -> Option<u64>;
    /// Memory the system could hand to a new model without swapping.
    fn available(&self) -> Option<u64>;
}

/// Everything a note is made with.
pub struct Deps {
    pub models: Arc<dyn ModelSource>,
    pub engines: Arc<dyn Engines>,
    pub memory: Arc<dyn Memory>,
    /// Transcript tokens per summary chunk; see `summary::chunk_tokens`.
    pub chunk_tokens: usize,
    /// How long to wait for memory to come back before the summary model
    /// loads; see [`MEMORY_WAIT`].
    pub memory_wait: Duration,
}

/// The transcription model's memory counts as released when the process is
/// back within this much of where it was before loading it.
pub const RELEASE_TOLERANCE_BYTES: u64 = 256 << 20;

/// macOS counts a dropped model's memory as free again a moment after the
/// process has let it go: 0.25 to 0.7 seconds after unloading Qwen3-ASR on
/// an M5 with 24 GB. The pipeline waits up to this long for it.
pub const MEMORY_WAIT: Duration = Duration::from_secs(5);
const MEMORY_POLL: Duration = Duration::from_millis(100);

/// A window this quiet holds no speech and is not transcribed. About
/// -80 dBFS, below any microphone's noise floor.
const SILENCE_RMS: f32 = 1e-4;

/// What the interface shows while a note is being written.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum Stage {
    /// Another note is being written first.
    Waiting,
    Transcribing {
        done_seconds: f64,
        total_seconds: f64,
    },
    /// `done` of `total` requests to the summary model.
    Summarizing { done: usize, total: usize },
}

/// Reported to the interface.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A recording's status changed.
    Changed {
        folder: PathBuf,
    },
    Progress {
        folder: PathBuf,
        stage: Stage,
    },
}

/// Numbers from one run, for the evaluation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub asr_model: String,
    pub summary_model: String,
    pub audio_seconds: f64,
    pub language: Option<String>,
    pub segments: Vec<SegmentReport>,
    pub summary: Summary,
    pub transcribe_seconds: f64,
    pub summarize_seconds: f64,
    /// Process footprint before the transcription model was loaded and after
    /// it was dropped.
    pub footprint_before: Option<u64>,
    pub footprint_after_unload: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SegmentReport {
    pub start_seconds: f64,
    pub text: String,
}

/// Runs every step for one recording folder and writes `note.md`. Does not
/// change `state.json`; [`Pipeline`] does that around it.
pub fn run(folder: &Path, deps: &Deps, on_stage: &mut dyn FnMut(Stage)) -> Result<Report, String> {
    let state = read_state(folder)
        .map_err(|err| format!("Anchovy can't read this recording's state. {err}"))?;
    let (asr, asr_ready) = deps.models.selected(Role::Transcribe);
    let (llm, llm_ready) = deps.models.selected(Role::Summarize);
    if !(asr_ready && llm_ready) {
        return Err(MODELS_MISSING.into());
    }
    let start = start_time(folder).ok_or("The recording folder is not named by its start time.")?;

    // 1. The audio, as the transcription model takes it.
    let audio = folder.join(Quality::High.audio_file_name());
    if !audio.is_file() {
        return Err(if folder.join(Quality::Small.audio_file_name()).is_file() {
            "Anchovy can't read M4A recordings yet.".into()
        } else {
            "The audio file is missing.".into()
        });
    }
    let samples = read_model_audio(&audio).map_err(|err| err.to_string())?;
    let rate = f64::from(MODEL_RATE);
    let audio_seconds = samples.len() as f64 / rate;

    // 2. Transcribe window by window.
    let started = Instant::now();
    let footprint_before = deps.memory.footprint();
    let mut transcriber = deps
        .engines
        .transcriber(&asr)
        .map_err(|err| format!("Anchovy couldn't load {}. {err}", asr.display_name))?;
    let mut joiner = Joiner::new();
    let mut languages = Vec::new();
    on_stage(Stage::Transcribing {
        done_seconds: 0.0,
        total_seconds: audio_seconds,
    });
    for (index, window) in windows::plan(samples.len(), MODEL_RATE, WINDOW_SECONDS, OVERLAP_SECONDS)
        .into_iter()
        .enumerate()
    {
        let chunk = &samples[window.start..window.end];
        if rms(chunk) >= SILENCE_RMS {
            let heard = transcriber
                .transcribe(chunk)
                .map_err(|err| format!("Transcription stopped. {err}"))?;
            joiner.push(index, window.start as f64 / rate, &heard.text);
            languages.push(heard.language);
        }
        on_stage(Stage::Transcribing {
            done_seconds: window.end as f64 / rate,
            total_seconds: audio_seconds,
        });
    }

    // 3. Unload it, and check the memory came back.
    drop(transcriber);
    let footprint_after_unload = deps.memory.footprint();
    let transcribe_seconds = started.elapsed().as_secs_f64();
    if let (Some(before), Some(after)) = (footprint_before, footprint_after_unload) {
        if after > before + RELEASE_TOLERANCE_BYTES {
            return Err(NOT_RELEASED.into());
        }
    }
    let segments = joiner.finish();
    if segments.is_empty() {
        return Err("No speech was heard in this recording.".into());
    }

    // 4. Summarize, only if the summary model fits in memory. A token is at
    // least one character, so a short transcript needs a smaller context
    // than a full chunk.
    let text = windows::plain_text(&segments);
    let language = summary::language_name(&languages, &text);
    let context = summary::context_tokens(deps.chunk_tokens.min(text.chars().count()));
    let needed = deps.engines.summarizer_bytes(&llm, context);
    if !wait_for_memory(deps.memory.as_ref(), needed, deps.memory_wait) {
        return Err(format!(
            "Not enough memory to write the summary. The summary model needs about {} GB \
             of free memory. Close other apps, then choose Retry.",
            gigabytes(needed)
        ));
    }
    let started = Instant::now();
    let mut summarizer = deps
        .engines
        .summarizer(&llm, context)
        .map_err(|err| format!("Anchovy couldn't load {}. {err}", llm.display_name))?;
    let lines: Vec<String> = segments
        .iter()
        .map(|segment| segment.text.clone())
        .collect();
    let result = summary::summarize(
        summarizer.as_mut(),
        &lines,
        language.as_deref(),
        deps.chunk_tokens,
        &mut |done, total| on_stage(Stage::Summarizing { done, total }),
    );
    drop(summarizer);
    let summary = result.map_err(|err| err.to_string())?;
    let summarize_seconds = started.elapsed().as_secs_f64();

    // 5. The note, all at once.
    let note = Note {
        start,
        duration_seconds: audio_seconds as u64,
        // The meeting prompt (plan step 7) is the only other source.
        source: Source::Manual,
        computer_audio: state.inputs.contains(&Input::ComputerAudio),
        quality: Quality::High,
        asr_model: asr.display_name.clone(),
        summary_model: llm.display_name.clone(),
        summary: summary.summary.clone(),
        decisions: summary.decisions.clone(),
        action_items: summary.action_items.clone(),
        transcript: segments
            .iter()
            .map(Segment::line)
            .collect::<Vec<_>>()
            .join("\n"),
    };
    write_note(folder, &note.render())
        .map_err(|err| format!("Anchovy couldn't write note.md. {err}"))?;

    Ok(Report {
        asr_model: asr.display_name,
        summary_model: llm.display_name,
        audio_seconds,
        language,
        segments: segments
            .into_iter()
            .map(|segment| SegmentReport {
                start_seconds: segment.start_seconds,
                text: segment.text,
            })
            .collect(),
        summary,
        transcribe_seconds,
        summarize_seconds,
        footprint_before,
        footprint_after_unload,
    })
}

const MODELS_MISSING: &str = "The models for this note are not on this Mac. Download them in \
                              Models, then choose Retry.";
const NOT_RELEASED: &str = "The speech model did not release its memory. Quit and reopen \
                            Anchovy, then choose Retry.";
const INTERRUPTED: &str = "Anchovy quit before the note was finished. Choose Retry to start again.";
const CRASHED: &str = "Anchovy stopped writing the note because of an internal error. Choose \
                       Retry to start again.";

/// Whether `needed` bytes are available, now or within `wait`.
fn wait_for_memory(memory: &dyn Memory, needed: u64, wait: Duration) -> bool {
    let started = Instant::now();
    loop {
        match memory.available() {
            None => return true,
            Some(available) if available >= needed => return true,
            Some(_) if started.elapsed() >= wait => return false,
            Some(_) => thread::sleep(MEMORY_POLL.min(wait)),
        }
    }
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Decimal gigabytes, as Finder counts them, rounded up to a tenth.
fn gigabytes(bytes: u64) -> String {
    format!("{:.1}", (bytes as f64 / 1e8).ceil() / 10.0)
}

/// The start time from the folder name, `yyyy-MM-dd-HHmm` with an optional
/// `-2`, `-3`. The name keeps only the minute, so seconds are 0.
fn start_time(folder: &Path) -> Option<StartTime> {
    let name = folder.file_name()?.to_str()?;
    let base = name.get(..15)?;
    let bytes = base.as_bytes();
    let shape = bytes.iter().enumerate().all(|(i, b)| match i {
        4 | 7 | 10 => *b == b'-',
        _ => b.is_ascii_digit(),
    });
    if !shape {
        return None;
    }
    let num = |range: std::ops::Range<usize>| base[range].parse().ok();
    Some(StartTime {
        year: num(0..4)?,
        month: num(5..7)? as u8,
        day: num(8..10)? as u8,
        hour: num(11..13)? as u8,
        minute: num(13..15)? as u8,
        second: 0,
    })
}

/// Settings the pipeline reads. The Settings screen (plan step 8) edits
/// them; until then the file only exists if written by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoteSettings {
    /// Start a note as soon as a recording stops. On by default.
    pub generate_notes_automatically: bool,
}

impl Default for NoteSettings {
    fn default() -> Self {
        NoteSettings {
            generate_notes_automatically: true,
        }
    }
}

pub const SETTINGS_FILE: &str = "settings.json";

/// Reads `settings.json` from the app's support folder. A missing or
/// unreadable file means the defaults.
pub fn read_settings(dir: &Path) -> NoteSettings {
    fs::read(dir.join(SETTINGS_FILE))
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_default()
}

#[derive(Debug)]
pub enum PipelineError {
    /// The note is already being written or waiting its turn.
    Busy,
    /// The recording has not stopped yet.
    Recording,
    /// `note.md` exists and the user did not agree to replace it.
    NoteExists,
    /// Retry or Regenerate while a selected model is not on this Mac.
    ModelsMissing(Vec<String>),
    Notes(NotesError),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Busy => write!(f, "Anchovy is already writing this note."),
            PipelineError::Recording => write!(f, "This recording has not stopped yet."),
            PipelineError::NoteExists => write!(f, "This recording already has a note."),
            PipelineError::ModelsMissing(names) => {
                write!(f, "Download {} in Models first.", names.join(" and "))
            }
            PipelineError::Notes(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for PipelineError {}

impl From<NotesError> for PipelineError {
    fn from(err: NotesError) -> Self {
        PipelineError::Notes(err)
    }
}

#[derive(Default)]
struct Queue {
    waiting: VecDeque<PathBuf>,
    running: Option<PathBuf>,
    stages: HashMap<PathBuf, Stage>,
    worker: bool,
}

impl Queue {
    fn has(&self, folder: &Path) -> bool {
        self.running.as_deref() == Some(folder) || self.waiting.iter().any(|f| f == folder)
    }
}

struct Inner {
    deps: Deps,
    queue: Mutex<Queue>,
    idle: Condvar,
    on_event: Box<dyn Fn(Event) + Send + Sync>,
}

impl Inner {
    fn changed(&self, folder: &Path) {
        (self.on_event)(Event::Changed {
            folder: folder.to_path_buf(),
        });
    }

    /// Display names of the selected models that are not on this Mac.
    fn missing_models(&self) -> Vec<String> {
        Role::ALL
            .iter()
            .map(|role| self.deps.models.selected(*role))
            .filter(|(_, ready)| !ready)
            .map(|(model, _)| model.display_name)
            .collect()
    }

    /// The worker thread: writes queued notes until none are left.
    fn work(&self) {
        loop {
            let folder = {
                let mut queue = self.queue.lock().unwrap();
                match queue.waiting.pop_front() {
                    Some(folder) => {
                        queue.running = Some(folder.clone());
                        folder
                    }
                    None => {
                        queue.worker = false;
                        self.idle.notify_all();
                        return;
                    }
                }
            };
            self.write_one(&folder);
            let mut queue = self.queue.lock().unwrap();
            queue.running = None;
            queue.stages.remove(&folder);
        }
    }

    fn write_one(&self, folder: &Path) {
        let mut on_stage = |stage: Stage| {
            self.queue
                .lock()
                .unwrap()
                .stages
                .insert(folder.to_path_buf(), stage.clone());
            (self.on_event)(Event::Progress {
                folder: folder.to_path_buf(),
                stage,
            });
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(folder, &self.deps, &mut on_stage)
        }))
        .unwrap_or_else(|_| Err(CRASHED.into()));
        let saved = read_state(folder).and_then(|mut state| {
            match result {
                Ok(report) => {
                    state.asr_model = Some(report.asr_model);
                    state.summary_model = Some(report.summary_model);
                    state.move_to(Status::Ready)?;
                }
                Err(reason) => state.move_to(Status::Failed { reason })?,
            }
            write_state(folder, &state)
        });
        if let Err(err) = saved {
            eprintln!(
                "Anchovy couldn't save the state of {}. {err}",
                folder.display()
            );
        }
        self.changed(folder);
    }
}

/// `state.json`, or for a folder written before it existed, the state the
/// library shows for it.
fn read_or_infer_state(folder: &Path, has_note: bool) -> Result<State, NotesError> {
    match read_state(folder) {
        Err(NotesError::Io(err)) if err.kind() == io::ErrorKind::NotFound => Ok(State {
            status: if has_note {
                Status::Ready
            } else {
                Status::Saved
            },
            ..State::new()
        }),
        result => result,
    }
}

/// Writes notes one at a time on a worker thread.
pub struct Pipeline {
    inner: Arc<Inner>,
}

impl Pipeline {
    pub fn new(deps: Deps, on_event: impl Fn(Event) + Send + Sync + 'static) -> Self {
        Pipeline {
            inner: Arc::new(Inner {
                deps,
                queue: Mutex::new(Queue::default()),
                idle: Condvar::new(),
                on_event: Box::new(on_event),
            }),
        }
    }

    /// Generate note, Retry, and Regenerate note. Moves the recording to
    /// Working and queues it, or to Needs models if a model is missing.
    /// `replace` is the user's answer to "Replace note.md?".
    pub fn generate(&self, folder: &Path, replace: bool) -> Result<Status, PipelineError> {
        let mut queue = self.inner.queue.lock().unwrap();
        if queue.has(folder) {
            return Err(PipelineError::Busy);
        }
        let has_note = folder.join(NOTE_FILE).is_file();
        let mut state = read_or_infer_state(folder, has_note)?;
        match state.status {
            Status::Recording => return Err(PipelineError::Recording),
            Status::Working => return Err(PipelineError::Busy),
            _ => {}
        }
        if has_note && !replace {
            return Err(PipelineError::NoteExists);
        }
        let missing = self.inner.missing_models();
        if !missing.is_empty() {
            return match state.status {
                Status::Saved => {
                    state.move_to(Status::NeedsModels)?;
                    write_state(folder, &state)?;
                    self.inner.changed(folder);
                    Ok(Status::NeedsModels)
                }
                Status::NeedsModels => Ok(Status::NeedsModels),
                _ => Err(PipelineError::ModelsMissing(missing)),
            };
        }
        state.move_to(Status::Working)?;
        write_state(folder, &state)?;
        queue.waiting.push_back(folder.to_path_buf());
        queue.stages.insert(folder.to_path_buf(), Stage::Waiting);
        self.inner.changed(folder);
        if !queue.worker {
            queue.worker = true;
            let inner = self.inner.clone();
            thread::spawn(move || inner.work());
        }
        Ok(Status::Working)
    }

    /// Called when a recording stops. Starts its note only when notes are
    /// generated automatically; otherwise the recording stays Saved.
    pub fn after_recording(
        &self,
        folder: &Path,
        automatic: bool,
    ) -> Result<Option<Status>, PipelineError> {
        if !automatic {
            return Ok(None);
        }
        self.generate(folder, false).map(Some)
    }

    /// Starts the notes that waited for models, once the models are here.
    pub fn resume_waiting(&self, folders: &[PathBuf]) {
        if !self.inner.missing_models().is_empty() {
            return;
        }
        for folder in folders {
            if read_state(folder).is_ok_and(|state| state.status == Status::NeedsModels) {
                if let Err(err) = self.generate(folder, false) {
                    eprintln!(
                        "Anchovy couldn't start the note in {}. {err}",
                        folder.display()
                    );
                }
            }
        }
    }

    /// At launch: a recording left Working by an app that quit is Failed.
    pub fn recover(&self, folders: &[PathBuf]) {
        let queue = self.inner.queue.lock().unwrap();
        for folder in folders {
            if queue.has(folder) {
                continue;
            }
            let Ok(mut state) = read_state(folder) else {
                continue;
            };
            if state.status != Status::Working {
                continue;
            }
            let failed = Status::Failed {
                reason: INTERRUPTED.into(),
            };
            if state.move_to(failed).is_ok() && write_state(folder, &state).is_ok() {
                self.inner.changed(folder);
            }
        }
    }

    /// Where a queued or running note is.
    pub fn stage(&self, folder: &Path) -> Option<Stage> {
        self.inner.queue.lock().unwrap().stages.get(folder).cloned()
    }

    /// Blocks until no note is queued or running.
    pub fn wait_idle(&self) {
        let mut queue = self.inner.queue.lock().unwrap();
        while queue.worker {
            queue = self.inner.idle.wait(queue).unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::{EngineError, Heard, Prompt, Summarizer, Transcriber};
    use crate::notes::folder::create_recording_folder;
    use crate::notes::note::NOTE_TMP_FILE;
    use crate::notes::test_dir::TestDir;
    use crate::notes::APP_DIR;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    const START: StartTime = StartTime {
        year: 2026,
        month: 9,
        day: 26,
        hour: 14,
        minute: 10,
        second: 41,
    };

    const VALID: &str = r#"{"summary": "We planned the launch.", "decisions": ["Ship on Friday."], "action_items": ["Sam writes the release notes."]}"#;

    // --- Fakes -----------------------------------------------------------

    #[derive(Default)]
    struct Log(Mutex<Vec<String>>);

    impl Log {
        fn push(&self, entry: impl Into<String>) {
            self.0.lock().unwrap().push(entry.into());
        }
        fn entries(&self) -> Vec<String> {
            self.0.lock().unwrap().clone()
        }
    }

    struct FakeModels {
        transcribe_ready: AtomicBool,
        summarize_ready: AtomicBool,
    }

    impl FakeModels {
        fn ready() -> Arc<Self> {
            Arc::new(FakeModels {
                transcribe_ready: AtomicBool::new(true),
                summarize_ready: AtomicBool::new(true),
            })
        }
    }

    impl ModelSource for FakeModels {
        fn selected(&self, role: Role) -> (ModelFiles, bool) {
            let (id, name, ready) = match role {
                Role::Transcribe => ("asr", "Qwen3-ASR 1.7B", &self.transcribe_ready),
                Role::Summarize => ("llm", "Qwen3-4B-Instruct-2507", &self.summarize_ready),
            };
            let model = ModelFiles {
                id: id.into(),
                engine: "fake".into(),
                display_name: name.into(),
                files: vec![PathBuf::from(format!("/models/{id}.gguf"))],
                size_bytes: 2_000_000_000,
            };
            (model, ready.load(Ordering::SeqCst))
        }
    }

    /// Answers each window in turn from `heard`, and each summary request
    /// from `answers`. Checks the two models are never loaded together.
    struct FakeEngines {
        heard: Mutex<VecDeque<Heard>>,
        answers: Mutex<VecDeque<String>>,
        log: Arc<Log>,
        live: Arc<AtomicUsize>,
        /// Called during every transcription, to look at the folder.
        during: Mutex<Option<Box<dyn Fn() + Send>>>,
    }

    impl FakeEngines {
        fn new(heard: &[&str], answers: &[&str]) -> Arc<Self> {
            Arc::new(FakeEngines {
                heard: Mutex::new(
                    heard
                        .iter()
                        .map(|text| Heard {
                            language: Some("English".into()),
                            text: text.to_string(),
                        })
                        .collect(),
                ),
                answers: Mutex::new(answers.iter().map(|a| a.to_string()).collect()),
                log: Arc::new(Log::default()),
                live: Arc::new(AtomicUsize::new(0)),
                during: Mutex::new(None),
            })
        }

        fn answer(&self, answer: &str) {
            self.answers.lock().unwrap().push_back(answer.into());
        }

        fn hear(&self, text: &str) {
            self.heard.lock().unwrap().push_back(Heard {
                language: Some("English".into()),
                text: text.into(),
            });
        }
    }

    struct FakeTranscriber {
        engines: Arc<FakeEngines>,
    }

    impl Transcriber for FakeTranscriber {
        fn transcribe(&mut self, samples: &[f32]) -> Result<Heard, EngineError> {
            self.engines.log.push(format!(
                "transcribe {:.0} s",
                samples.len() as f64 / 16_000.0
            ));
            if let Some(during) = self.engines.during.lock().unwrap().as_ref() {
                during();
            }
            Ok(self
                .engines
                .heard
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_default())
        }
    }

    impl Drop for FakeTranscriber {
        fn drop(&mut self) {
            self.engines.live.fetch_sub(1, Ordering::SeqCst);
            self.engines.log.push("unload transcriber");
        }
    }

    struct FakeSummarizer {
        engines: Arc<FakeEngines>,
    }

    impl Summarizer for FakeSummarizer {
        fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
            Ok(text.split_whitespace().count())
        }

        fn complete(&mut self, prompt: &Prompt, attempt: u32) -> Result<String, EngineError> {
            let part = prompt.user.lines().next().unwrap_or_default().to_string();
            self.engines
                .log
                .push(format!("complete attempt {attempt}: {part}"));
            self.engines
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| EngineError::Run("no answer".into()))
        }
    }

    impl Drop for FakeSummarizer {
        fn drop(&mut self) {
            self.engines.live.fetch_sub(1, Ordering::SeqCst);
            self.engines.log.push("unload summarizer");
        }
    }

    /// The pipeline holds `Arc<dyn Engines>`; the fakes need their own Arc
    /// to hand to what they load.
    struct Shared(Arc<FakeEngines>);

    impl Engines for Shared {
        fn transcriber(&self, model: &ModelFiles) -> Result<Box<dyn Transcriber>, EngineError> {
            assert_eq!(
                self.0.live.fetch_add(1, Ordering::SeqCst),
                0,
                "two models loaded"
            );
            self.0.log.push(format!("load {}", model.id));
            Ok(Box::new(FakeTranscriber {
                engines: self.0.clone(),
            }))
        }

        fn summarizer(
            &self,
            model: &ModelFiles,
            context_tokens: u32,
        ) -> Result<Box<dyn Summarizer>, EngineError> {
            assert_eq!(
                self.0.live.fetch_add(1, Ordering::SeqCst),
                0,
                "two models loaded"
            );
            self.0
                .log
                .push(format!("load {} with {context_tokens} tokens", model.id));
            Ok(Box::new(FakeSummarizer {
                engines: self.0.clone(),
            }))
        }

        fn summarizer_bytes(&self, model: &ModelFiles, context_tokens: u32) -> u64 {
            model.size_bytes + u64::from(context_tokens) * 1000
        }
    }

    /// Readings in order. The last `available` reading repeats.
    struct FakeMemory {
        footprints: Mutex<VecDeque<u64>>,
        available: Mutex<VecDeque<u64>>,
    }

    impl FakeMemory {
        fn new(footprints: &[u64], available: &[u64]) -> Arc<Self> {
            Arc::new(FakeMemory {
                footprints: Mutex::new(footprints.iter().copied().collect()),
                available: Mutex::new(available.iter().copied().collect()),
            })
        }

        fn plenty() -> Arc<Self> {
            Self::new(&[], &[16 << 30])
        }
    }

    impl Memory for FakeMemory {
        fn footprint(&self) -> Option<u64> {
            // Without readings, the footprint never changes.
            Some(
                self.footprints
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or(500 << 20),
            )
        }

        fn available(&self) -> Option<u64> {
            let mut readings = self.available.lock().unwrap();
            if readings.len() > 1 {
                readings.pop_front()
            } else {
                readings.front().copied()
            }
        }
    }

    struct Setup {
        _dir: TestDir,
        folder: PathBuf,
        engines: Arc<FakeEngines>,
        models: Arc<FakeModels>,
        events: Arc<Mutex<Vec<Event>>>,
        pipeline: Pipeline,
    }

    fn deps(
        models: Arc<FakeModels>,
        engines: Arc<FakeEngines>,
        memory: Arc<FakeMemory>,
        chunk_tokens: usize,
    ) -> Deps {
        Deps {
            models,
            engines: Arc::new(Shared(engines)),
            memory,
            chunk_tokens,
            memory_wait: Duration::from_millis(300),
        }
    }

    /// A Saved recording of `seconds` of a quiet tone, with both inputs.
    fn saved_recording(notes_dir: &Path, seconds: usize) -> PathBuf {
        let folder = create_recording_folder(notes_dir, &START).unwrap();
        let samples: Vec<u8> = (0..seconds * 16_000)
            .flat_map(|i| {
                let s = 0.1 * (i as f32 * 0.05).sin();
                ((s * 32767.0) as i16).to_le_bytes()
            })
            .collect();
        fs::write(folder.join("audio.wav"), wav(16_000, &samples)).unwrap();
        let mut state = State::new();
        state.inputs = vec![Input::Microphone, Input::ComputerAudio];
        state.move_to(Status::Saved).unwrap();
        write_state(&folder, &state).unwrap();
        folder
    }

    fn wav(rate: u32, data: &[u8]) -> Vec<u8> {
        let mut out = b"RIFF".to_vec();
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(b"\x02\0\x10\0data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn setup_with(
        seconds: usize,
        engines: Arc<FakeEngines>,
        memory: Arc<FakeMemory>,
        chunk_tokens: usize,
    ) -> Setup {
        let dir = TestDir::new();
        let folder = saved_recording(dir.path(), seconds);
        let models = FakeModels::ready();
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let pipeline = Pipeline::new(
            deps(models.clone(), engines.clone(), memory, chunk_tokens),
            move |event| sink.lock().unwrap().push(event),
        );
        Setup {
            _dir: dir,
            folder,
            engines,
            models,
            events,
            pipeline,
        }
    }

    /// 70 seconds: three windows.
    fn setup(answers: &[&str]) -> Setup {
        setup_with(
            70,
            FakeEngines::new(
                &[
                    "Okay, let's start. We will ship on Friday and then",
                    "on Friday and then test the build.",
                    "Sam writes the release notes.",
                ],
                answers,
            ),
            FakeMemory::plenty(),
            24_000,
        )
    }

    fn status(folder: &Path) -> Status {
        read_state(folder).unwrap().status
    }

    fn note(folder: &Path) -> Option<String> {
        fs::read_to_string(folder.join(NOTE_FILE)).ok()
    }

    fn failed_reason(folder: &Path) -> String {
        match status(folder) {
            Status::Failed { reason } => reason,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    // --- Ready -----------------------------------------------------------

    #[test]
    fn a_saved_recording_becomes_a_ready_note() {
        let s = setup(&[VALID]);

        assert_eq!(
            s.pipeline.generate(&s.folder, false).unwrap(),
            Status::Working
        );
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        let expected = "\
---
date: 2026-09-26T14:10:00
duration: 00:01:10
source: manual
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

## Action items

- Sam writes the release notes.

## Transcript

00:00:00 Okay, let's start. We will ship on Friday and then
00:00:27 test the build.
00:00:54 Sam writes the release notes.

## Audio

[audio.wav](audio.wav)
";
        assert_eq!(note(&s.folder).unwrap(), expected);
        let state = read_state(&s.folder).unwrap();
        assert_eq!(state.asr_model.as_deref(), Some("Qwen3-ASR 1.7B"));
        assert_eq!(
            state.summary_model.as_deref(),
            Some("Qwen3-4B-Instruct-2507")
        );
        assert!(!s.folder.join(APP_DIR).join(NOTE_TMP_FILE).exists());
    }

    #[test]
    fn the_audio_is_transcribed_in_overlapping_windows_and_models_take_turns() {
        let s = setup(&[VALID]);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(
            s.engines.log.entries(),
            [
                "load asr",
                "transcribe 30 s",
                "transcribe 30 s",
                "transcribe 16 s",
                "unload transcriber",
                // 96 characters of transcript, at most 96 tokens, plus room
                // for the prompt and the answer.
                "load llm with 3168 tokens",
                "complete attempt 0: Transcript:",
                "unload summarizer",
            ]
        );
    }

    #[test]
    fn the_recording_is_working_while_its_note_is_written() {
        let s = setup(&[VALID]);
        let folder = s.folder.clone();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_in = seen.clone();
        *s.engines.during.lock().unwrap() = Some(Box::new(move || {
            seen_in.lock().unwrap().push(status(&folder));
        }));

        s.pipeline.generate(&s.folder, false).unwrap();
        assert_eq!(status(&s.folder), Status::Working);
        s.pipeline.wait_idle();

        assert_eq!(*seen.lock().unwrap(), vec![Status::Working; 3]);
        let events = s.events.lock().unwrap().clone();
        let changed = events
            .iter()
            .filter(|e| matches!(e, Event::Changed { .. }))
            .count();
        assert_eq!(changed, 2, "Working, then Ready: {events:?}");
        let stages: Vec<Stage> = events
            .into_iter()
            .filter_map(|e| match e {
                Event::Progress { stage, .. } => Some(stage),
                _ => None,
            })
            .collect();
        assert_eq!(
            stages,
            [
                Stage::Transcribing {
                    done_seconds: 0.0,
                    total_seconds: 70.0
                },
                Stage::Transcribing {
                    done_seconds: 30.0,
                    total_seconds: 70.0
                },
                Stage::Transcribing {
                    done_seconds: 57.0,
                    total_seconds: 70.0
                },
                Stage::Transcribing {
                    done_seconds: 70.0,
                    total_seconds: 70.0
                },
                Stage::Summarizing { done: 0, total: 1 },
                Stage::Summarizing { done: 1, total: 1 },
            ]
        );
        assert_eq!(s.pipeline.stage(&s.folder), None);
    }

    #[test]
    fn a_long_transcript_is_summarized_in_chunks() {
        let part = r#"{"summary": "Part.", "decisions": [], "action_items": []}"#;
        // Fake tokens are words. The windows hold 10, 7, and 6 words, so a
        // 17-token budget makes two chunks: [10] and [7, 6].
        let s = setup_with(
            70,
            FakeEngines::new(
                &[
                    "one two three four five six seven eight nine ten",
                    "eleven twelve thirteen fourteen fifteen sixteen seventeen",
                    "eighteen nineteen twenty twenty-one twenty-two twenty-three",
                ],
                &[part, part, VALID],
            ),
            FakeMemory::plenty(),
            17,
        );

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        let log = s.engines.log.entries();
        // The transcript is longer than a chunk: the context holds a chunk.
        assert!(
            log.contains(&"load llm with 3089 tokens".to_string()),
            "{log:?}"
        );
        let requests: Vec<&String> = log.iter().filter(|e| e.starts_with("complete")).collect();
        assert_eq!(
            requests,
            [
                "complete attempt 0: This is part 1 of 2 of one meeting. Write notes for this part only.",
                "complete attempt 0: This is part 2 of 2 of one meeting. Write notes for this part only.",
                "complete attempt 0: Notes from each part, in order:",
            ]
        );
        assert!(note(&s.folder)
            .unwrap()
            .contains("## Summary\n\nWe planned the launch.\n"));
    }

    // --- Needs models ----------------------------------------------------

    #[test]
    fn without_the_models_the_note_waits_and_starts_when_they_arrive() {
        let s = setup(&[VALID]);
        s.models.summarize_ready.store(false, Ordering::SeqCst);

        assert_eq!(
            s.pipeline.generate(&s.folder, false).unwrap(),
            Status::NeedsModels
        );
        s.pipeline.wait_idle();
        assert_eq!(status(&s.folder), Status::NeedsModels);
        assert_eq!(note(&s.folder), None);
        assert!(s.engines.log.entries().is_empty());

        // Still missing: nothing happens.
        s.pipeline.resume_waiting(std::slice::from_ref(&s.folder));
        s.pipeline.wait_idle();
        assert_eq!(status(&s.folder), Status::NeedsModels);

        s.models.summarize_ready.store(true, Ordering::SeqCst);
        s.pipeline.resume_waiting(std::slice::from_ref(&s.folder));
        s.pipeline.wait_idle();
        assert_eq!(status(&s.folder), Status::Ready);
    }

    #[test]
    fn retry_and_regenerate_say_which_models_to_download() {
        let s = setup(&[VALID]);
        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();
        s.models.transcribe_ready.store(false, Ordering::SeqCst);
        s.models.summarize_ready.store(false, Ordering::SeqCst);

        let err = s.pipeline.generate(&s.folder, true).unwrap_err();

        assert_eq!(
            err.to_string(),
            "Download Qwen3-ASR 1.7B and Qwen3-4B-Instruct-2507 in Models first."
        );
        assert_eq!(status(&s.folder), Status::Ready);
    }

    #[test]
    fn models_removed_before_the_note_starts_fail_it_with_a_reason() {
        let s = setup(&[VALID]);
        let deps = deps(
            s.models.clone(),
            s.engines.clone(),
            FakeMemory::plenty(),
            24_000,
        );
        // Deleted in Models after the note was queued.
        s.models.summarize_ready.store(false, Ordering::SeqCst);

        let err = run(&s.folder, &deps, &mut |_| {}).unwrap_err();

        assert_eq!(
            err,
            "The models for this note are not on this Mac. Download them in Models, then \
             choose Retry."
        );
        assert!(s.engines.log.entries().is_empty());
    }

    // --- Failed and Retry ------------------------------------------------

    #[test]
    fn a_bad_answer_is_asked_for_once_more() {
        let s = setup(&["Here are your notes!", VALID]);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        let attempts: Vec<String> = s
            .engines
            .log
            .entries()
            .into_iter()
            .filter(|e| e.starts_with("complete"))
            .collect();
        assert_eq!(
            attempts,
            [
                "complete attempt 0: Transcript:",
                "complete attempt 1: Transcript:"
            ]
        );
    }

    #[test]
    fn a_meeting_that_mixes_chinese_and_english_may_be_summarized_in_either() {
        // Chinese first, then mostly English: the speech model reports
        // English, and the summary model may still answer in Chinese.
        let mixed = "大家好，很高兴认识大家，我叫小王。I work remotely from Berlin, and I'm \
                     happy to join the team and learn from all of you.";
        let chinese = r#"{"summary": "小王做了自我介绍，很高兴加入团队。", "decisions": [], "action_items": []}"#;
        let english = r#"{"summary": "Xiao Wang introduced himself and is happy to join the team.", "decisions": [], "action_items": []}"#;
        for answer in [chinese, english] {
            let s = setup_with(
                20,
                FakeEngines::new(&[mixed], &[answer]),
                FakeMemory::plenty(),
                24_000,
            );

            s.pipeline.generate(&s.folder, false).unwrap();
            s.pipeline.wait_idle();

            assert_eq!(status(&s.folder), Status::Ready, "{answer}");
        }
    }

    #[test]
    fn two_bad_answers_fail_the_note_and_keep_the_audio() {
        let s = setup(&["not json", "still not json"]);
        let audio = fs::read(s.folder.join("audio.wav")).unwrap();

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(
            failed_reason(&s.folder),
            "The summary model did not return a valid note. The answer is not JSON."
        );
        assert_eq!(note(&s.folder), None);
        assert!(!s.folder.join(APP_DIR).join(NOTE_TMP_FILE).exists());
        assert_eq!(fs::read(s.folder.join("audio.wav")).unwrap(), audio);
    }

    #[test]
    fn retry_after_a_failure_writes_the_note() {
        let s = setup(&["not json", "not json"]);
        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();
        assert!(matches!(status(&s.folder), Status::Failed { .. }));

        s.engines.hear("We ship on Friday.");
        s.engines.answer(VALID);
        assert_eq!(
            s.pipeline.generate(&s.folder, false).unwrap(),
            Status::Working
        );
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        assert!(note(&s.folder).unwrap().contains("- Ship on Friday."));
    }

    #[test]
    fn the_speech_model_must_release_its_memory_before_the_summary_model_loads() {
        // Before loading, then after unloading: 1 GB still held.
        let memory = FakeMemory::new(&[500 << 20, 1524 << 20], &[16 << 30]);
        let s = setup_with(70, FakeEngines::new(&["Hello."], &[VALID]), memory, 24_000);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(
            failed_reason(&s.folder),
            "The speech model did not release its memory. Quit and reopen Anchovy, then \
             choose Retry."
        );
        assert!(!s
            .engines
            .log
            .entries()
            .iter()
            .any(|e| e.starts_with("load llm")));
        assert_eq!(note(&s.folder), None);
    }

    #[test]
    fn a_little_memory_kept_after_unloading_is_fine() {
        let memory = FakeMemory::new(&[500 << 20, 700 << 20], &[16 << 30]);
        let s = setup_with(70, FakeEngines::new(&["Hello."], &[VALID]), memory, 24_000);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
    }

    #[test]
    fn without_enough_free_memory_the_summary_model_is_not_loaded() {
        // The fake needs 2 GB and 1 KB per context token, about 2.003 GB.
        let memory = FakeMemory::new(&[], &[2_000_000_000]);
        let s = setup_with(70, FakeEngines::new(&["Hello."], &[VALID]), memory, 24_000);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(
            failed_reason(&s.folder),
            "Not enough memory to write the summary. The summary model needs about 2.1 GB \
             of free memory. Close other apps, then choose Retry."
        );
        assert!(!s
            .engines
            .log
            .entries()
            .iter()
            .any(|e| e.starts_with("load llm")));
    }

    #[test]
    fn memory_the_system_is_still_reclaiming_is_waited_for() {
        // Right after a model is dropped, macOS takes a moment to count its
        // memory as free again.
        let memory = FakeMemory::new(&[], &[1 << 30, 1 << 30, 16 << 30]);
        let s = setup_with(70, FakeEngines::new(&["Hello."], &[VALID]), memory, 24_000);

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
    }

    #[test]
    fn a_silent_recording_fails_without_loading_the_summary_model() {
        let dir = TestDir::new();
        let folder = saved_recording(dir.path(), 5);
        let silence = wav(16_000, &vec![0u8; 5 * 16_000 * 2]);
        fs::write(folder.join("audio.wav"), silence).unwrap();
        let engines = FakeEngines::new(&[], &[VALID]);
        let pipeline = Pipeline::new(
            deps(
                FakeModels::ready(),
                engines.clone(),
                FakeMemory::plenty(),
                24_000,
            ),
            |_| {},
        );

        pipeline.generate(&folder, false).unwrap();
        pipeline.wait_idle();

        assert_eq!(
            failed_reason(&folder),
            "No speech was heard in this recording."
        );
        assert!(!engines
            .log
            .entries()
            .iter()
            .any(|e| e.starts_with("load llm")));
    }

    #[test]
    fn audio_that_cannot_be_read_fails_with_the_reason() {
        let s = setup(&[VALID]);
        fs::write(s.folder.join("audio.wav"), b"not audio").unwrap();

        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();

        assert_eq!(
            failed_reason(&s.folder),
            "Anchovy can't read this audio: not a WAV file."
        );
    }

    #[test]
    fn a_recording_left_working_by_a_quit_app_is_failed_at_launch() {
        let s = setup(&[VALID]);
        let mut state = read_state(&s.folder).unwrap();
        state.move_to(Status::Working).unwrap();
        write_state(&s.folder, &state).unwrap();

        s.pipeline.recover(std::slice::from_ref(&s.folder));

        assert_eq!(
            failed_reason(&s.folder),
            "Anchovy quit before the note was finished. Choose Retry to start again."
        );
    }

    // --- Regenerate ------------------------------------------------------

    #[test]
    fn regenerate_replaces_note_md_only_when_the_user_agreed() {
        let s = setup(&[VALID]);
        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();
        fs::write(s.folder.join(NOTE_FILE), "My edited note.").unwrap();

        let err = s.pipeline.generate(&s.folder, false).unwrap_err();
        assert!(matches!(err, PipelineError::NoteExists));
        assert_eq!(note(&s.folder).unwrap(), "My edited note.");
        assert_eq!(status(&s.folder), Status::Ready);

        s.engines.hear("We ship on Monday instead.");
        s.engines.answer(
            r#"{"summary": "We moved the launch.", "decisions": ["Ship on Monday."], "action_items": []}"#,
        );
        assert_eq!(
            s.pipeline.generate(&s.folder, true).unwrap(),
            Status::Working
        );
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        let text = note(&s.folder).unwrap();
        assert!(text.contains("- Ship on Monday."), "{text}");
        assert!(text.contains("## Action items\n\n-\n"), "{text}");
    }

    #[test]
    fn a_failed_regenerate_keeps_the_old_note() {
        let s = setup(&[VALID]);
        s.pipeline.generate(&s.folder, false).unwrap();
        s.pipeline.wait_idle();
        let before = note(&s.folder).unwrap();

        s.engines.hear("Hello.");
        s.engines.answer("oops");
        s.engines.answer("oops");
        s.pipeline.generate(&s.folder, true).unwrap();
        s.pipeline.wait_idle();

        assert!(matches!(status(&s.folder), Status::Failed { .. }));
        assert_eq!(note(&s.folder).unwrap(), before);
    }

    #[test]
    fn a_note_already_queued_is_not_queued_twice() {
        let s = setup(&[VALID]);
        let release = Arc::new(Mutex::new(()));
        let held = release.lock().unwrap();
        let wait = release.clone();
        *s.engines.during.lock().unwrap() = Some(Box::new(move || drop(wait.lock().unwrap())));

        s.pipeline.generate(&s.folder, false).unwrap();
        let err = s.pipeline.generate(&s.folder, true).unwrap_err();
        assert!(matches!(err, PipelineError::Busy));
        drop(held);
        s.pipeline.wait_idle();
        assert_eq!(status(&s.folder), Status::Ready);
    }

    #[test]
    fn a_recording_still_recording_cannot_get_a_note() {
        let s = setup(&[VALID]);
        write_state(&s.folder, &State::new()).unwrap();
        let err = s.pipeline.generate(&s.folder, false).unwrap_err();
        assert!(matches!(err, PipelineError::Recording));
    }

    // --- Generate notes automatically ------------------------------------

    #[test]
    fn with_automatic_notes_off_stopping_writes_no_note() {
        let s = setup(&[VALID]);

        assert_eq!(s.pipeline.after_recording(&s.folder, false).unwrap(), None);
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Saved);
        assert_eq!(note(&s.folder), None);
        assert!(s.engines.log.entries().is_empty());
    }

    #[test]
    fn with_automatic_notes_on_stopping_writes_the_note() {
        let s = setup(&[VALID]);

        assert_eq!(
            s.pipeline.after_recording(&s.folder, true).unwrap(),
            Some(Status::Working)
        );
        s.pipeline.wait_idle();

        assert_eq!(status(&s.folder), Status::Ready);
        assert!(note(&s.folder).is_some());
    }

    #[test]
    fn automatic_notes_are_on_unless_turned_off() {
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
}
