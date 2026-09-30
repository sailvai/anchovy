//! Recording logic that does not touch the system: which microphone to use,
//! how the aggregate device's streams split into microphone and computer
//! audio, getting samples out of the audio callback without locks, and a
//! session that writes the file and moves the recording from Recording to
//! Saved. `mac.rs` supplies the Core Audio half through [`Capture`].

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;

use super::file_writer::WavWriter;
use super::mixer::{Mixer, OUTPUT_RATE};
use super::small::{self, Encoder};
use crate::notes::folder::{create_recording_folder, Quality, StartTime};
use crate::notes::note::Source;
use crate::notes::state::{write_state, Input, State, Status};
use crate::settings::RecordingOptions;

/// Core Audio objects Anchovy creates (the private aggregate device) have UIDs
/// starting with this, so they never show up as an input device to choose.
pub const OWN_DEVICE_UID_PREFIX: &str = "com.sailvai.anchovy.";

/// The most input streams the audio callback handles. Microphones have one
/// or two; the system-audio tap adds one.
pub const MAX_STREAMS: usize = 16;

/// How much audio the ring buffers hold if the writer thread falls behind.
const RING_SECONDS: f64 = 4.0;

/// How often the interface hears about elapsed time and file size.
pub const PROGRESS_INTERVAL: Duration = Duration::from_secs(1);

/// How often the writer thread empties the ring buffers.
const DRAIN_INTERVAL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    pub uid: String,
    /// The name macOS shows for the device.
    pub name: String,
}

/// Devices to offer the user, in system order, without Anchovy's own.
pub fn listable(devices: Vec<InputDevice>) -> Vec<InputDevice> {
    devices
        .into_iter()
        .filter(|device| !device.uid.starts_with(OWN_DEVICE_UID_PREFIX))
        .collect()
}

/// The chosen device if it is still connected, otherwise the system default
/// input, otherwise the first one.
pub fn pick_microphone<'a>(
    devices: &'a [InputDevice],
    wanted_uid: Option<&str>,
    default_uid: Option<&str>,
) -> Option<&'a InputDevice> {
    let find = |uid: Option<&str>| uid.and_then(|uid| devices.iter().find(|d| d.uid == uid));
    find(wanted_uid)
        .or_else(|| find(default_uid))
        .or_else(|| devices.first())
}

/// The second line of the sources area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ComputerAudio {
    Recording,
    /// Shown as "Not allowed". The recording has the microphone only.
    NotAllowed,
}

impl ComputerAudio {
    /// What goes into `inputs` for a recording made this way.
    pub fn inputs(self) -> Vec<Input> {
        match self {
            ComputerAudio::Recording => vec![Input::Microphone, Input::ComputerAudio],
            ComputerAudio::NotAllowed => vec![Input::Microphone],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingError {
    NoMicrophone,
    AlreadyRecording,
    NotRecording,
    Device(String),
    Disk(String),
}

impl fmt::Display for RecordingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordingError::NoMicrophone => write!(f, "No microphone is connected."),
            RecordingError::AlreadyRecording => write!(f, "A recording is already running."),
            RecordingError::NotRecording => write!(f, "Nothing is recording."),
            RecordingError::Device(err) => write!(f, "The microphone could not start: {err}"),
            RecordingError::Disk(err) => write!(f, "Could not write the recording: {err}"),
        }
    }
}

impl std::error::Error for RecordingError {}

impl From<io::Error> for RecordingError {
    fn from(err: io::Error) -> Self {
        RecordingError::Disk(err.to_string())
    }
}

/// Channel count of each input stream of the capture device, split into the
/// microphone's streams and the system-audio tap's. An aggregate device lists
/// its main sub-device (the microphone) first, then its taps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamLayout {
    pub microphone: Vec<usize>,
    pub computer: Vec<usize>,
}

impl StreamLayout {
    pub fn split(streams: &[usize], microphone_streams: usize) -> Result<Self, RecordingError> {
        if streams.len() > MAX_STREAMS {
            return Err(RecordingError::Device(format!(
                "the device has {} input streams; Anchovy handles up to {MAX_STREAMS}",
                streams.len()
            )));
        }
        let split = microphone_streams.min(streams.len());
        let layout = StreamLayout {
            microphone: streams[..split].to_vec(),
            computer: streams[split..].to_vec(),
        };
        if layout.microphone.iter().sum::<usize>() == 0 {
            return Err(RecordingError::Device("it has no input channels".into()));
        }
        Ok(layout)
    }

    pub fn has_computer_audio(&self) -> bool {
        self.computer.iter().sum::<usize>() > 0
    }
}

/// The average of every channel of `buffers` at `frame`. Empty buffers (Core
/// Audio passes a null buffer for a stream with nothing to deliver) count as
/// silence.
fn mono(buffers: &[&[f32]], channels: &[usize], frame: usize) -> f32 {
    let mut sum = 0.0;
    let mut count = 0;
    for (buffer, &ch) in buffers.iter().zip(channels) {
        if buffer.is_empty() || ch == 0 {
            continue;
        }
        for c in 0..ch {
            sum += buffer[frame * ch + c];
        }
        count += ch;
    }
    if count == 0 {
        0.0
    } else {
        sum / count as f32
    }
}

fn frames(buffers: &[&[f32]], channels: &[usize]) -> Option<usize> {
    buffers
        .iter()
        .zip(channels)
        .filter(|(buffer, &ch)| !buffer.is_empty() && ch > 0)
        .map(|(buffer, &ch)| buffer.len() / ch)
        .min()
}

/// The audio callback's side: downmixes each cycle to one mono sample per
/// source and pushes it into a ring buffer. Never locks or allocates.
pub struct Feed {
    layout: StreamLayout,
    microphone: Producer<f32>,
    computer: Option<Producer<f32>>,
    dropped: Arc<AtomicU64>,
}

/// The writer thread's side of the ring buffers.
pub struct Drain {
    microphone: Consumer<f32>,
    computer: Option<Consumer<f32>>,
    dropped: Arc<AtomicU64>,
}

impl Drain {
    pub fn computer_audio(&self) -> ComputerAudio {
        if self.computer.is_some() {
            ComputerAudio::Recording
        } else {
            ComputerAudio::NotAllowed
        }
    }
}

/// Ring buffers for a device running at `sample_rate` with `layout`.
pub fn rings(sample_rate: f64, layout: StreamLayout) -> (Feed, Drain) {
    let capacity = (sample_rate * RING_SECONDS) as usize;
    let (mic_in, mic_out) = RingBuffer::new(capacity);
    let (computer_in, computer_out) = if layout.has_computer_audio() {
        let (tx, rx) = RingBuffer::new(capacity);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let dropped = Arc::new(AtomicU64::new(0));
    (
        Feed {
            layout,
            microphone: mic_in,
            computer: computer_in,
            dropped: dropped.clone(),
        },
        Drain {
            microphone: mic_out,
            computer: computer_out,
            dropped,
        },
    )
}

impl Feed {
    /// One IO cycle: `buffers` are the device's input buffers in stream order.
    /// Both sources get the same number of frames, so they stay aligned; if
    /// the writer has fallen far behind, the frames that do not fit are
    /// dropped and counted.
    pub fn deliver(&mut self, buffers: &[&[f32]]) {
        let Feed {
            layout,
            microphone,
            computer,
            dropped,
        } = self;
        let split = layout.microphone.len().min(buffers.len());
        let (mic_buffers, rest) = buffers.split_at(split);
        let computer_buffers = &rest[..layout.computer.len().min(rest.len())];

        let Some(available) = [
            frames(mic_buffers, &layout.microphone),
            frames(computer_buffers, &layout.computer),
        ]
        .into_iter()
        .flatten()
        .min() else {
            return;
        };
        let mut n = available.min(microphone.slots());
        if let Some(computer) = computer.as_ref() {
            n = n.min(computer.slots());
        }
        if n < available {
            dropped.fetch_add((available - n) as u64, Ordering::Relaxed);
        }
        if n == 0 {
            return;
        }
        if let Ok(chunk) = microphone.write_chunk_uninit(n) {
            chunk.fill_from_iter((0..n).map(|f| mono(mic_buffers, &layout.microphone, f)));
        }
        if let Some(computer) = computer {
            if let Ok(chunk) = computer.write_chunk_uninit(n) {
                chunk.fill_from_iter((0..n).map(|f| mono(computer_buffers, &layout.computer, f)));
            }
        }
    }
}

/// Level of one source over the whole recording, before mixing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Level {
    pub peak: f32,
    pub rms: f32,
    #[serde(skip)]
    sum_squares: f64,
    #[serde(skip)]
    count: u64,
}

impl Level {
    fn add(&mut self, samples: &[f32]) {
        for &s in samples {
            self.peak = self.peak.max(s.abs());
            self.sum_squares += (s as f64) * (s as f64);
        }
        self.count += samples.len() as u64;
        if self.count > 0 {
            self.rms = (self.sum_squares / self.count as f64).sqrt() as f32;
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Levels {
    pub microphone: Level,
    /// `None` when computer audio was not recorded.
    pub computer: Option<Level>,
}

/// `inputs` for a finished recording: what actually reached the file.
///
/// A tap that delivered nothing but digital zeros counts as not recorded. A
/// denied or unanswered System Audio Recording permission gives exactly that,
/// and so does an allowed tap while nothing plays; no public API tells the two
/// apart, and either way no computer audio is in the file.
pub fn recorded_inputs(levels: &Levels) -> Vec<Input> {
    match levels.computer {
        Some(level) if level.peak > 0.0 => ComputerAudio::Recording.inputs(),
        _ => ComputerAudio::NotAllowed.inputs(),
    }
}

/// Moves samples from the ring buffers through the mixer into the file.
pub struct Pump<W: Write + Seek> {
    drain: Drain,
    mixer: Mixer,
    writer: WavWriter<W>,
    levels: Levels,
    microphone: Vec<f32>,
    computer: Vec<f32>,
    mixed: Vec<f32>,
}

fn read_all(consumer: &mut Consumer<f32>, n: usize, into: &mut Vec<f32>) {
    into.clear();
    if let Ok(chunk) = consumer.read_chunk(n) {
        let (first, second) = chunk.as_slices();
        into.extend_from_slice(first);
        into.extend_from_slice(second);
        chunk.commit_all();
    }
}

impl<W: Write + Seek> Pump<W> {
    pub fn new(drain: Drain, input_rate: f64, writer: WavWriter<W>) -> Self {
        let computer_audio = drain.computer.is_some();
        Pump {
            mixer: Mixer::new(input_rate, computer_audio),
            levels: Levels {
                microphone: Level::default(),
                computer: computer_audio.then(Level::default),
            },
            drain,
            writer,
            microphone: Vec::new(),
            computer: Vec::new(),
            mixed: Vec::new(),
        }
    }

    /// Writes everything both sources have delivered so far.
    pub fn pump(&mut self) -> io::Result<()> {
        let mut n = self.drain.microphone.slots();
        if let Some(computer) = &self.drain.computer {
            n = n.min(computer.slots());
        }
        if n == 0 {
            return Ok(());
        }
        read_all(&mut self.drain.microphone, n, &mut self.microphone);
        self.levels.microphone.add(&self.microphone);
        let computer = match (&mut self.drain.computer, &mut self.levels.computer) {
            (Some(consumer), Some(level)) => {
                read_all(consumer, n, &mut self.computer);
                level.add(&self.computer);
                Some(&self.computer[..])
            }
            _ => None,
        };
        self.mixed.clear();
        self.mixer
            .process(&self.microphone, computer, &mut self.mixed);
        self.writer.write(&self.mixed)
    }

    pub fn progress(&self) -> Progress {
        Progress {
            seconds: self.writer.seconds(),
            bytes: self.writer.bytes(),
        }
    }

    pub fn update_header(&mut self) -> io::Result<()> {
        self.writer.update_header()
    }

    pub fn levels(&self) -> Levels {
        self.levels
    }

    pub fn dropped(&self) -> u64 {
        self.drain.dropped.load(Ordering::Relaxed)
    }

    pub fn finish(self) -> io::Result<(W, Progress, Levels, u64)> {
        let progress = self.progress();
        let dropped = self.drain.dropped.load(Ordering::Relaxed);
        let levels = self.levels;
        Ok((self.writer.finish()?, progress, levels, dropped))
    }
}

/// Pushed to the interface while recording.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Progress {
    /// Seconds of audio in the file.
    pub seconds: f64,
    /// Size of the audio file.
    pub bytes: u64,
}

/// The running Core Audio half of a recording.
pub trait Capture: Send {
    /// Stops delivering audio and releases the devices. After this returns,
    /// no more samples reach the ring buffers.
    fn stop(self: Box<Self>) -> Result<(), RecordingError>;
}

/// How one IO proc's teardown went. `stopped` only counts when `started`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IoTeardown {
    pub started: bool,
    pub stopped: bool,
    pub destroyed: bool,
}

impl IoTeardown {
    /// Whether the IO proc's context may be freed. Core Audio keeps calling a
    /// registered, running IO proc with that pointer, so if stopping or
    /// unregistering failed, the context must be kept alive for good.
    pub fn may_free_context(self) -> bool {
        (!self.started || self.stopped) && self.destroyed
    }
}

/// A capture that has started, as returned by `mac::start`.
pub struct Started {
    pub capture: Box<dyn Capture>,
    pub sample_rate: f64,
    pub drain: Drain,
    pub microphone: String,
}

/// What the interface shows while recording.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recording {
    pub folder: PathBuf,
    pub microphone: String,
    pub computer_audio: ComputerAudio,
    /// Read from the settings when the recording started.
    pub quality: Quality,
}

/// A finished recording.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Saved {
    pub folder: PathBuf,
    pub audio: PathBuf,
    pub seconds: f64,
    pub bytes: u64,
    pub inputs: Vec<Input>,
    pub levels: Levels,
    /// Frames lost because the writer fell behind. Normally 0.
    pub dropped_frames: u64,
}

struct Finished {
    progress: Progress,
    levels: Levels,
    dropped: u64,
    /// A write that failed during the recording; the audio before it is kept.
    error: Option<io::Error>,
}

/// Always returns what was recorded: a failure to write or to close the file
/// is carried in `Finished::error`, so the recording can still be Saved.
fn run_writer(
    mut pump: Pump<BufWriter<File>>,
    stop: Arc<AtomicBool>,
    tick: Duration,
    on_progress: impl Fn(Progress),
) -> Finished {
    let mut error = None;
    let mut next_tick = Instant::now() + tick;
    loop {
        // Read the flag before draining: the capture has stopped by the time
        // it is set, so this last drain gets every sample.
        let stopping = stop.load(Ordering::Acquire);
        if error.is_none() {
            if let Err(err) = pump.pump() {
                error = Some(err);
            }
        }
        if Instant::now() >= next_tick {
            next_tick += tick;
            if error.is_none() {
                if let Err(err) = pump.update_header() {
                    error = Some(err);
                }
            }
            on_progress(pump.progress());
        }
        if stopping {
            break;
        }
        thread::sleep(DRAIN_INTERVAL.min(tick));
    }
    let (progress, levels, dropped) = (pump.progress(), pump.levels(), pump.dropped());
    let closed = pump.finish().and_then(|(file, ..)| {
        file.into_inner()
            .map_err(|err| err.into_error())?
            .sync_all()
    });
    Finished {
        progress,
        levels,
        dropped,
        error: error.or(closed.err()),
    }
}

/// Moves a stopped recording to Saved with the inputs that reached the file,
/// then reports the first error, if any. The audio is kept either way: its
/// header was last updated at most a second before the failure.
fn save(
    folder: &Path,
    audio: &Path,
    state: State,
    finished: Finished,
    stopped: Result<(), RecordingError>,
) -> Result<Saved, RecordingError> {
    let state = mark_saved(folder, &state, &finished.levels)?;
    report(folder, audio, state, finished, stopped)
}

/// Writes `state` as Saved, with the inputs that reached the file. The state
/// passed in is left as it was if that fails.
fn mark_saved(folder: &Path, state: &State, levels: &Levels) -> Result<State, RecordingError> {
    let mut state = state.clone();
    state.inputs = recorded_inputs(levels);
    state
        .move_to(Status::Saved)
        .map_err(|err| RecordingError::Disk(err.to_string()))?;
    write_state(folder, &state).map_err(|err| RecordingError::Disk(err.to_string()))?;
    Ok(state)
}

/// The first error, if any, once the recording is Saved.
fn report(
    folder: &Path,
    audio: &Path,
    state: State,
    finished: Finished,
    stopped: Result<(), RecordingError>,
) -> Result<Saved, RecordingError> {
    if let Some(err) = finished.error {
        return Err(err.into());
    }
    stopped?;
    Ok(Saved {
        folder: folder.to_path_buf(),
        audio: audio.to_path_buf(),
        seconds: finished.progress.seconds,
        bytes: finished.progress.bytes,
        inputs: state.inputs,
        levels: finished.levels,
        dropped_frames: finished.dropped,
    })
}

/// One recording from start to Saved.
pub struct Session {
    folder: PathBuf,
    /// The file being written: `audio.wav`, or for Small the WAV in
    /// `.anchovy/` that becomes `audio.m4a` on stop.
    audio: PathBuf,
    quality: Quality,
    state: State,
    capture: Box<dyn Capture>,
    stop: Arc<AtomicBool>,
    writer: JoinHandle<Finished>,
}

impl Session {
    /// Creates the recording folder, marks it Recording with the inputs being
    /// captured, and starts writing the WAV: `audio.wav` for High, the
    /// hidden one for Small. If any of that fails, the capture is stopped.
    /// `stop` later corrects the inputs to what reached the file.
    pub fn start(
        notes_dir: &Path,
        start: StartTime,
        source: Source,
        quality: Quality,
        started: Started,
        tick: Duration,
        on_progress: impl Fn(Progress) + Send + 'static,
    ) -> Result<(Session, Recording), RecordingError> {
        let Started {
            capture,
            sample_rate,
            drain,
            microphone,
        } = started;
        let computer_audio = drain.computer_audio();
        let prepared = (|| {
            let folder = create_recording_folder(notes_dir, &start)?;
            let mut state = State::new();
            state.inputs = computer_audio.inputs();
            state.source = source;
            write_state(&folder, &state).map_err(|err| RecordingError::Disk(err.to_string()))?;
            // write_state made `.anchovy/`, where Small's WAV goes.
            let audio = small::recording_path(&folder, quality);
            let writer = WavWriter::new(BufWriter::new(File::create(&audio)?), OUTPUT_RATE)?;
            Ok::<_, RecordingError>((folder, audio, state, writer))
        })();
        let (folder, audio, state, writer) = match prepared {
            Ok(prepared) => prepared,
            Err(err) => {
                let _ = capture.stop();
                return Err(err);
            }
        };
        let pump = Pump::new(drain, sample_rate, writer);
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let stop = stop.clone();
            thread::spawn(move || run_writer(pump, stop, tick, on_progress))
        };
        let recording = Recording {
            folder: folder.clone(),
            microphone,
            computer_audio,
            quality,
        };
        Ok((
            Session {
                folder,
                audio,
                quality,
                state,
                capture,
                stop,
                writer,
            },
            recording,
        ))
    }

    /// Stops the capture, closes the file, and moves the recording to Saved.
    /// Small is encoded to `audio.m4a` first with `encoder`; if that fails
    /// the audio is kept as `audio.wav` and `state.json` says why. If a write
    /// failed along the way, the audio up to that point is kept and Saved,
    /// and the error is returned.
    pub fn stop(self, encoder: &dyn Encoder) -> Result<Saved, RecordingError> {
        let Session {
            folder,
            mut audio,
            quality,
            state,
            capture,
            stop,
            writer,
        } = self;
        let stopped = capture.stop();
        stop.store(true, Ordering::Release);
        // If the writer thread died, what reached the file is unknown, so
        // Levels::default() makes `inputs` claim the microphone only.
        let mut finished = writer.join().unwrap_or_else(|_| Finished {
            progress: Progress {
                seconds: 0.0,
                bytes: 0,
            },
            levels: Levels::default(),
            dropped: 0,
            error: Some(io::Error::other("the writer stopped unexpectedly")),
        });
        if quality != Quality::Small {
            return save(&folder, &audio, state, finished, stopped);
        }
        // Small: state.json says Saved before the WAV leaves `.anchovy/`.
        let levels = finished.levels;
        let mut written = None;
        let done = small::finish(&folder, encoder, |done| {
            let mut next = state.clone();
            next.encoding_failed = done.failure.clone();
            let saved = mark_saved(&folder, &next, &levels)
                .map_err(|err| io::Error::other(err.to_string()))?;
            written = Some(saved);
            Ok(())
        });
        match done {
            Ok(done) => {
                finished.progress.bytes = std::fs::metadata(&done.audio)
                    .map(|meta| meta.len())
                    .unwrap_or(finished.progress.bytes);
                audio = done.audio;
            }
            // The WAV stays in `.anchovy/`, and the next launch tries again.
            Err(err) => finished.error = finished.error.or(Some(err)),
        }
        match written {
            Some(state) => report(&folder, &audio, state, finished, stopped),
            // Encoding could not start, or state.json could not be written:
            // try to mark the recording Saved as High would be.
            None => save(&folder, &audio, state, finished, stopped),
        }
    }
}

/// At most one recording at a time, shared by the interface commands. The
/// computer audio check (`probe`) never overlaps a recording: Record stops a
/// running check and waits for it, and no check starts while recording.
pub struct Recorder {
    /// Turns a Small recording into `audio.m4a` when it stops.
    encoder: Arc<dyn Encoder>,
    session: Mutex<Option<Session>>,
    computer_audio: Mutex<Option<ComputerAudio>>,
    /// Held by a running check, and by `start` until its session exists.
    probing: Mutex<()>,
    /// Set by `start` to cut a running check short.
    stop_probe: AtomicBool,
}

impl Recorder {
    pub fn new(encoder: Arc<dyn Encoder>) -> Self {
        Recorder {
            encoder,
            session: Mutex::default(),
            computer_audio: Mutex::default(),
            probing: Mutex::default(),
            stop_probe: AtomicBool::default(),
        }
    }

    /// Starts a recording with `options`, read from the settings once, now:
    /// `capture` gets the input device to open, and a change to the settings
    /// after this does not reach this recording.
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        notes_dir: &Path,
        start: StartTime,
        source: Source,
        options: RecordingOptions,
        capture: impl FnOnce(Option<&str>) -> Result<Started, RecordingError>,
        tick: Duration,
        on_progress: impl Fn(Progress) + Send + 'static,
    ) -> Result<Recording, RecordingError> {
        self.stop_probe.store(true, Ordering::SeqCst);
        let _no_probe = self.probing.lock().unwrap();
        self.stop_probe.store(false, Ordering::SeqCst);
        let mut session = self.session.lock().unwrap();
        if session.is_some() {
            return Err(RecordingError::AlreadyRecording);
        }
        let (started, recording) = Session::start(
            notes_dir,
            start,
            source,
            options.quality,
            capture(options.input_device.as_deref())?,
            tick,
            on_progress,
        )?;
        *self.computer_audio.lock().unwrap() = Some(recording.computer_audio);
        *session = Some(started);
        Ok(recording)
    }

    pub fn stop(&self) -> Result<Saved, RecordingError> {
        let session = self
            .session
            .lock()
            .unwrap()
            .take()
            .ok_or(RecordingError::NotRecording)?;
        session.stop(self.encoder.as_ref())
    }

    pub fn is_recording(&self) -> bool {
        self.session.lock().unwrap().is_some()
    }

    /// Runs the computer audio check, which gets a `keep_going` test to poll
    /// so Record can cut it short. `None` if it did not run because a
    /// recording is running, or was cut short and its answer is not reliable.
    pub fn probe<T>(&self, run: impl FnOnce(&dyn Fn() -> bool) -> T) -> Option<T> {
        let _probing = self.probing.lock().unwrap();
        if self.is_recording() {
            return None;
        }
        let keep_going = || !self.stop_probe.load(Ordering::SeqCst);
        let answer = run(&keep_going);
        keep_going().then_some(answer)
    }

    /// Whether the last recording got computer audio. `None` before the
    /// first recording of this run.
    pub fn last_computer_audio(&self) -> Option<ComputerAudio> {
        *self.computer_audio.lock().unwrap()
    }
}

/// The computer audio check plays this tone from Anchovy's own process into a
/// muted tap of that process, so nothing reaches the speakers. About -60 dBFS,
/// in case the output is not muted.
pub const PROBE_AMPLITUDE: f32 = 0.001;
const PROBE_FREQUENCY: f32 = 440.0;

/// A sine tone for the computer audio check. Runs on the audio thread.
pub struct Tone {
    phase: f32,
    step: f32,
}

impl Tone {
    pub fn new(sample_rate: f64) -> Self {
        Tone {
            phase: 0.0,
            step: std::f32::consts::TAU * PROBE_FREQUENCY / sample_rate as f32,
        }
    }

    /// Fills one interleaved output buffer of `channels` channels; every
    /// channel gets the same sample. Call `advance` once per IO cycle after
    /// filling every buffer, so all buffers start at the same phase.
    pub fn fill(&self, out: &mut [f32], channels: usize) {
        for (i, frame) in out.chunks_mut(channels.max(1)).enumerate() {
            let sample = PROBE_AMPLITUDE * (self.phase + self.step * i as f32).sin();
            frame.fill(sample);
        }
    }

    pub fn advance(&mut self, frames: usize) {
        self.phase = (self.phase + self.step * frames as f32) % std::f32::consts::TAU;
    }
}

/// Whether the tap's buffers, which follow the `before_tap` input buffers of
/// the aggregate device's main sub-device, hold anything but digital silence. A denied or unanswered computer audio
/// permission gives a tap of zeros.
pub fn tap_heard(buffers: &[&[f32]], before_tap: usize) -> bool {
    buffers
        .get(before_tap..)
        .unwrap_or_default()
        .iter()
        .any(|buffer| buffer.iter().any(|&sample| sample != 0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::state::read_state;
    use crate::notes::test_dir::TestDir;
    use crate::recording::mixer::signal::{sine, tone_amplitude};
    use crate::recording::small::fake::CopyEncoder;
    use crate::settings::{read_settings, Settings, SettingsStore};
    use std::io::Cursor;
    use std::sync::mpsc;

    fn device(uid: &str, name: &str) -> InputDevice {
        InputDevice {
            uid: uid.into(),
            name: name.into(),
        }
    }

    const START: StartTime = StartTime {
        year: 2026,
        month: 9,
        day: 28,
        hour: 14,
        minute: 10,
        second: 0,
    };

    #[test]
    fn anchovys_own_aggregate_device_is_not_offered() {
        let devices = listable(vec![
            device("BuiltInMicrophoneDevice", "MacBook Air Microphone"),
            device("com.sailvai.anchovy.recording.42", "Anchovy"),
            device("AppleUSBAudioEngine:DJI", "Wireless Mic Rx"),
        ]);
        let names: Vec<_> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["MacBook Air Microphone", "Wireless Mic Rx"]);
    }

    #[test]
    fn the_chosen_microphone_wins_then_the_default_then_the_first() {
        let devices = [device("a", "A"), device("b", "B"), device("c", "C")];
        let pick = |wanted, default| pick_microphone(&devices, wanted, default).map(|d| &d.uid[..]);
        assert_eq!(pick(Some("c"), Some("b")), Some("c"));
        assert_eq!(pick(Some("unplugged"), Some("b")), Some("b"));
        assert_eq!(pick(None, Some("b")), Some("b"));
        assert_eq!(pick(None, None), Some("a"));
        assert_eq!(pick_microphone(&[], Some("a"), Some("a")), None);
    }

    #[test]
    fn without_computer_audio_inputs_are_microphone_only() {
        assert_eq!(ComputerAudio::NotAllowed.inputs(), vec![Input::Microphone]);
        assert_eq!(
            ComputerAudio::Recording.inputs(),
            vec![Input::Microphone, Input::ComputerAudio]
        );
        assert_eq!(
            serde_json::to_value(ComputerAudio::NotAllowed).unwrap(),
            "not_allowed"
        );
    }

    #[test]
    fn the_microphone_streams_come_first_then_the_tap() {
        let layout = StreamLayout::split(&[2, 2], 1).unwrap();
        assert_eq!(layout.microphone, vec![2]);
        assert_eq!(layout.computer, vec![2]);
        assert!(layout.has_computer_audio());

        let mic_only = StreamLayout::split(&[1, 1], 2).unwrap();
        assert_eq!(mic_only.microphone, vec![1, 1]);
        assert!(!mic_only.has_computer_audio());
    }

    #[test]
    fn an_aggregate_without_the_tap_stream_has_no_computer_audio() {
        let layout = StreamLayout::split(&[2], 1).unwrap();
        assert!(!layout.has_computer_audio());
    }

    #[test]
    fn a_device_without_input_channels_is_refused() {
        assert!(StreamLayout::split(&[0], 1).is_err());
        assert!(StreamLayout::split(&[], 1).is_err());
        assert!(StreamLayout::split(&[1; MAX_STREAMS + 1], 1).is_err());
    }

    fn drain_all(consumer: &mut Consumer<f32>) -> Vec<f32> {
        let mut out = Vec::new();
        read_all(consumer, consumer.slots(), &mut out);
        out
    }

    #[test]
    fn each_source_is_downmixed_to_mono() {
        let (mut feed, mut drain) = rings(48_000.0, StreamLayout::split(&[2, 2], 1).unwrap());
        feed.deliver(&[&[0.2, 0.4, 0.6, 0.8], &[1.0, 0.0, -0.5, -0.5]]);
        let mic = drain_all(&mut drain.microphone);
        assert_eq!(mic.len(), 2);
        assert!((mic[0] - 0.3).abs() < 1e-6 && (mic[1] - 0.7).abs() < 1e-6);
        assert_eq!(drain_all(drain.computer.as_mut().unwrap()), vec![0.5, -0.5]);
        assert_eq!(drain.computer_audio(), ComputerAudio::Recording);
    }

    #[test]
    fn a_microphone_split_over_several_streams_is_averaged() {
        let (mut feed, mut drain) = rings(48_000.0, StreamLayout::split(&[1, 1], 2).unwrap());
        feed.deliver(&[&[0.2, 0.4], &[0.4, 0.0]]);
        let got = drain_all(&mut drain.microphone);
        assert!((got[0] - 0.3).abs() < 1e-6 && (got[1] - 0.2).abs() < 1e-6);
        assert!(drain.computer.is_none());
        assert_eq!(drain.computer_audio(), ComputerAudio::NotAllowed);
    }

    #[test]
    fn a_missing_tap_buffer_counts_as_silence_and_keeps_the_sources_aligned() {
        let (mut feed, mut drain) = rings(48_000.0, StreamLayout::split(&[1, 2], 1).unwrap());
        feed.deliver(&[&[0.5, 0.5], &[]]);
        assert_eq!(drain_all(&mut drain.microphone), vec![0.5, 0.5]);
        assert_eq!(drain_all(drain.computer.as_mut().unwrap()), vec![0.0, 0.0]);
    }

    #[test]
    fn when_the_writer_falls_behind_extra_frames_are_dropped_and_counted() {
        // 4 seconds at 1 Hz: room for 4 frames.
        let (mut feed, mut drain) = rings(1.0, StreamLayout::split(&[1, 1], 1).unwrap());
        feed.deliver(&[&[0.1, 0.2, 0.3], &[0.1, 0.2, 0.3]]);
        feed.deliver(&[&[0.4, 0.5, 0.6], &[0.4, 0.5, 0.6]]);
        assert_eq!(drain.dropped.load(Ordering::Relaxed), 2);
        assert_eq!(drain_all(&mut drain.microphone), vec![0.1, 0.2, 0.3, 0.4]);
        assert_eq!(
            drain_all(drain.computer.as_mut().unwrap()),
            vec![0.1, 0.2, 0.3, 0.4]
        );
    }

    #[test]
    fn the_pump_writes_the_mix_of_both_sources_and_measures_each() {
        let (mut feed, drain) = rings(48_000.0, StreamLayout::split(&[1, 2], 1).unwrap());
        let writer = WavWriter::new(Cursor::new(Vec::new()), OUTPUT_RATE).unwrap();
        let mut pump = Pump::new(drain, 48_000.0, writer);
        let voice = sine(300.0, 0.3, 48_000, 1.0);
        let tone: Vec<f32> = sine(1_000.0, 0.3, 48_000, 1.0)
            .iter()
            .flat_map(|&s| [s, s])
            .collect();
        for (v, t) in voice.chunks(512).zip(tone.chunks(1024)) {
            feed.deliver(&[v, t]);
            pump.pump().unwrap();
        }
        assert_eq!(pump.progress().seconds, 1.0);
        assert_eq!(pump.progress().bytes, 44 + 96_000);

        let (file, _, levels, dropped) = pump.finish().unwrap();
        assert_eq!(dropped, 0);
        assert!((levels.microphone.peak - 0.3).abs() < 0.001);
        assert!((levels.computer.unwrap().rms - 0.3 / 2f32.sqrt()).abs() < 0.001);
        let samples: Vec<f32> = file.into_inner()[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32767.0)
            .collect();
        assert!((tone_amplitude(&samples, 300.0, 48_000) - 0.3).abs() < 0.01);
        assert!((tone_amplitude(&samples, 1_000.0, 48_000) - 0.3).abs() < 0.01);
    }

    /// Stands in for Core Audio: a thread that delivers 10 ms of a sine per
    /// source every 10 ms until stopped.
    struct FakeCapture {
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        stopped: Arc<AtomicBool>,
    }

    impl Capture for FakeCapture {
        fn stop(mut self: Box<Self>) -> Result<(), RecordingError> {
            self.stop.store(true, Ordering::Release);
            self.thread.take().unwrap().join().unwrap();
            self.stopped.store(true, Ordering::Release);
            Ok(())
        }
    }

    fn fake_capture(computer_audio: bool, stopped: Arc<AtomicBool>) -> Started {
        fake_capture_with(computer_audio, 0.3, stopped)
    }

    /// `computer_level` is the amplitude of the tone on the tap stream.
    fn fake_capture_with(
        computer_audio: bool,
        computer_level: f32,
        stopped: Arc<AtomicBool>,
    ) -> Started {
        let streams: &[usize] = if computer_audio { &[1, 2] } else { &[1] };
        let (mut feed, drain) = rings(48_000.0, StreamLayout::split(streams, 1).unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            thread::spawn(move || {
                let voice = sine(300.0, 0.3, 48_000, 0.01);
                let tone: Vec<f32> = sine(1_000.0, computer_level, 48_000, 0.01)
                    .iter()
                    .flat_map(|&s| [s, s])
                    .collect();
                while !stop.load(Ordering::Acquire) {
                    feed.deliver(&[&voice, &tone]);
                    thread::sleep(Duration::from_millis(10));
                }
            })
        };
        Started {
            capture: Box::new(FakeCapture {
                stop,
                thread: Some(thread),
                stopped,
            }),
            sample_rate: 48_000.0,
            drain,
            microphone: "MacBook Air Microphone".into(),
        }
    }

    const TICK: Duration = Duration::from_millis(50);

    /// High quality on the system default input.
    fn high() -> RecordingOptions {
        RecordingOptions {
            input_device: None,
            quality: Quality::High,
        }
    }

    #[test]
    fn a_device_chosen_during_a_recording_is_used_from_the_next_one() {
        let dir = TestDir::new();
        let support = TestDir::new();
        let settings = SettingsStore::load(support.path().to_path_buf());
        let usb = Settings {
            input_device: Some("AppleUSBAudioEngine:DJI".into()),
            ..Settings::default()
        };
        settings.update(usb).unwrap();
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        let asked = Arc::new(Mutex::new(Vec::new()));
        let capture = |asked: Arc<Mutex<Vec<Option<String>>>>| {
            move |device: Option<&str>| {
                asked.lock().unwrap().push(device.map(str::to_owned));
                Ok(fake_capture(false, Arc::new(AtomicBool::new(false))))
            }
        };

        recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                settings.recording_options(),
                capture(asked.clone()),
                TICK,
                |_| {},
            )
            .unwrap();
        settings
            .update(Settings {
                input_device: Some("BuiltInMicrophoneDevice".into()),
                ..Settings::default()
            })
            .unwrap();
        // The recording in progress keeps its device.
        assert_eq!(
            *asked.lock().unwrap(),
            [Some("AppleUSBAudioEngine:DJI".to_string())]
        );
        recorder.stop().unwrap();

        recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                settings.recording_options(),
                capture(asked.clone()),
                TICK,
                |_| {},
            )
            .unwrap();
        recorder.stop().unwrap();
        assert_eq!(
            *asked.lock().unwrap(),
            [
                Some("AppleUSBAudioEngine:DJI".to_string()),
                Some("BuiltInMicrophoneDevice".to_string())
            ]
        );
    }

    #[test]
    fn a_quality_chosen_during_a_recording_is_used_from_the_next_one() {
        let dir = TestDir::new();
        let support = TestDir::new();
        let settings = SettingsStore::load(support.path().to_path_buf());
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        let start = |recorder: &Recorder, minute| {
            recorder
                .start(
                    dir.path(),
                    StartTime { minute, ..START },
                    Source::Manual,
                    settings.recording_options(),
                    |_| Ok(fake_capture(true, Arc::new(AtomicBool::new(false)))),
                    TICK,
                    |_| {},
                )
                .unwrap()
        };

        let first = start(&recorder, 10);
        assert_eq!(first.quality, Quality::High);
        settings
            .update(Settings {
                recording_quality: Quality::Small,
                ..Settings::default()
            })
            .unwrap();
        thread::sleep(Duration::from_millis(100));
        let saved = recorder.stop().unwrap();
        // The recording in progress stays High.
        assert_eq!(saved.audio, first.folder.join("audio.wav"));
        assert!(!first.folder.join("audio.m4a").exists());

        let second = start(&recorder, 11);
        assert_eq!(second.quality, Quality::Small);
        // While it runs, the WAV is hidden in .anchovy/, and the size is what
        // is on disk.
        assert!(!second.folder.join("audio.wav").exists());
        assert!(second.folder.join(".anchovy/recording.wav").is_file());
        thread::sleep(Duration::from_millis(100));
        let saved = recorder.stop().unwrap();
        assert_eq!(saved.audio, second.folder.join("audio.m4a"));
        assert_eq!(
            saved.bytes,
            std::fs::metadata(&saved.audio).unwrap().len(),
            "the size of the M4A"
        );
        assert!(!second.folder.join(".anchovy/recording.wav").exists());
        let state = read_state(&second.folder).unwrap();
        assert_eq!(state.status, Status::Saved);
        assert_eq!(state.encoding_failed, None);
    }

    /// A folder where `state.tmp` should be, so the next `write_state` in
    /// this recording fails.
    fn block_state_writes(folder: &Path) {
        std::fs::create_dir(folder.join(".anchovy/state.tmp")).unwrap();
    }

    fn unblock_state_writes(folder: &Path) {
        std::fs::remove_dir(folder.join(".anchovy/state.tmp")).unwrap();
    }

    #[test]
    fn a_small_recording_whose_state_cannot_be_saved_keeps_its_wav_for_the_next_launch() {
        for encoder in [
            CopyEncoder::default(),
            CopyEncoder {
                fail: Some("No AAC encoder.".into()),
                ..CopyEncoder::default()
            },
        ] {
            let dir = TestDir::new();
            let recorder = Recorder::new(Arc::new(encoder));
            let small = RecordingOptions {
                input_device: None,
                quality: Quality::Small,
            };
            let recording = recorder
                .start(
                    dir.path(),
                    START,
                    Source::Manual,
                    small,
                    |_| Ok(fake_capture(true, Arc::new(AtomicBool::new(false)))),
                    TICK,
                    |_| {},
                )
                .unwrap();
            thread::sleep(Duration::from_millis(100));
            block_state_writes(&recording.folder);

            assert!(matches!(recorder.stop(), Err(RecordingError::Disk(_))));

            // state.json still says Recording, so the WAV must still be where
            // the next launch looks for it.
            assert_eq!(
                read_state(&recording.folder).unwrap().status,
                Status::Recording
            );
            assert!(recording.folder.join(".anchovy/recording.wav").is_file());

            unblock_state_writes(&recording.folder);
            let encoder = CopyEncoder::default();
            small::recover(&recording.folder, &encoder)
                .unwrap()
                .unwrap();
            assert_eq!(read_state(&recording.folder).unwrap().status, Status::Saved);
            assert!(recording.folder.join("audio.m4a").is_file());
            assert!(!recording.folder.join(".anchovy/recording.wav").exists());
        }
    }

    #[test]
    fn a_small_recording_whose_encode_fails_is_saved_as_wav_with_the_reason() {
        let dir = TestDir::new();
        let recorder = Recorder::new(Arc::new(CopyEncoder {
            fail: Some("No AAC encoder.".into()),
            ..CopyEncoder::default()
        }));
        let small = RecordingOptions {
            input_device: None,
            quality: Quality::Small,
        };
        let recording = recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                small,
                |_| Ok(fake_capture(true, Arc::new(AtomicBool::new(false)))),
                TICK,
                |_| {},
            )
            .unwrap();
        thread::sleep(Duration::from_millis(100));

        let saved = recorder.stop().unwrap();

        assert_eq!(saved.audio, recording.folder.join("audio.wav"));
        assert!(saved.seconds > 0.0);
        let state = read_state(&recording.folder).unwrap();
        assert_eq!(state.status, Status::Saved);
        assert_eq!(
            state.encoding_failed.as_deref(),
            Some("Anchovy couldn't save this recording as M4A, so it kept it as WAV. No AAC encoder.")
        );
    }

    #[test]
    fn progress_while_recording_small_is_the_size_of_the_file_on_disk() {
        let dir = TestDir::new();
        let (tx, rx) = mpsc::channel();
        let (session, recording) = Session::start(
            dir.path(),
            START,
            Source::Manual,
            Quality::Small,
            fake_capture(true, Arc::new(AtomicBool::new(false))),
            TICK,
            move |progress| tx.send(progress).unwrap(),
        )
        .unwrap();

        let mut progress = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        while progress.bytes <= 44 {
            progress = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        let on_disk = std::fs::metadata(recording.folder.join(".anchovy/recording.wav"))
            .unwrap()
            .len();
        assert!(on_disk >= progress.bytes, "{on_disk} < {}", progress.bytes);
        session.stop(&CopyEncoder::default()).unwrap();
    }

    #[test]
    fn a_saved_device_that_is_not_connected_records_with_the_default_and_stays_saved() {
        let dir = TestDir::new();
        let support = TestDir::new();
        let settings = SettingsStore::load(support.path().to_path_buf());
        let unplugged = Settings {
            input_device: Some("AppleUSBAudioEngine:DJI".into()),
            ..Settings::default()
        };
        settings.update(unplugged.clone()).unwrap();
        let connected = [
            device("BuiltInMicrophoneDevice", "MacBook Air Microphone"),
            device("ZoomAudioDevice", "Zoom Audio"),
        ];
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));

        let recording = recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                settings.recording_options(),
                // As mac::start picks the microphone.
                |wanted| {
                    let mic = pick_microphone(&connected, wanted, Some("BuiltInMicrophoneDevice"))
                        .ok_or(RecordingError::NoMicrophone)?;
                    Ok(Started {
                        microphone: mic.name.clone(),
                        ..fake_capture(false, Arc::new(AtomicBool::new(false)))
                    })
                },
                TICK,
                |_| {},
            )
            .unwrap();
        recorder.stop().unwrap();

        assert_eq!(recording.microphone, "MacBook Air Microphone");
        assert_eq!(settings.get(), unplugged);
        assert_eq!(read_settings(support.path()), unplugged);
    }

    #[test]
    fn a_recording_from_the_meeting_prompt_keeps_its_source() {
        let dir = TestDir::new();
        let (session, recording) = Session::start(
            dir.path(),
            START,
            Source::Meeting,
            Quality::High,
            fake_capture(true, Arc::new(AtomicBool::new(false))),
            TICK,
            |_| {},
        )
        .unwrap();
        assert_eq!(
            read_state(&recording.folder).unwrap().source,
            Source::Meeting
        );

        let saved = session.stop(&CopyEncoder::default()).unwrap();
        let state = read_state(&saved.folder).unwrap();
        assert_eq!(state.status, Status::Saved);
        assert_eq!(state.source, Source::Meeting);
    }

    #[test]
    fn a_session_is_recording_then_saved_with_the_file_closed() {
        let dir = TestDir::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let (session, recording) = Session::start(
            dir.path(),
            START,
            Source::Manual,
            Quality::High,
            fake_capture(true, stopped.clone()),
            TICK,
            move |progress| tx.send(progress).unwrap(),
        )
        .unwrap();

        assert_eq!(recording.folder, dir.path().join("2026-09-28-1410"));
        assert_eq!(recording.microphone, "MacBook Air Microphone");
        assert_eq!(recording.computer_audio, ComputerAudio::Recording);
        let state = read_state(&recording.folder).unwrap();
        assert_eq!(state.status, Status::Recording);
        assert_eq!(state.inputs, vec![Input::Microphone, Input::ComputerAudio]);

        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut last = first;
        while last.bytes <= first.bytes {
            last = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert!(last.seconds > first.seconds);

        let saved = session.stop(&CopyEncoder::default()).unwrap();
        assert!(stopped.load(Ordering::Acquire), "capture stopped");
        assert_eq!(saved.audio, recording.folder.join("audio.wav"));
        assert_eq!(read_state(&saved.folder).unwrap().status, Status::Saved);
        let bytes = std::fs::read(&saved.audio).unwrap();
        assert_eq!(bytes.len() as u64, saved.bytes);
        let data_len = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as u64;
        assert_eq!(
            data_len + 44,
            saved.bytes,
            "header closed with the full size"
        );
        assert!(saved.seconds >= last.seconds);
        assert_eq!(saved.inputs, vec![Input::Microphone, Input::ComputerAudio]);
        assert!(saved.levels.computer.unwrap().peak > 0.29);
    }

    #[test]
    fn without_computer_audio_the_recording_is_microphone_only() {
        let dir = TestDir::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let (session, recording) = Session::start(
            dir.path(),
            START,
            Source::Manual,
            Quality::High,
            fake_capture(false, stopped),
            TICK,
            |_| {},
        )
        .unwrap();
        assert_eq!(recording.computer_audio, ComputerAudio::NotAllowed);
        thread::sleep(Duration::from_millis(100));

        let saved = session.stop(&CopyEncoder::default()).unwrap();

        assert_eq!(saved.inputs, vec![Input::Microphone]);
        assert_eq!(saved.levels.computer, None);
        let state = read_state(&saved.folder).unwrap();
        assert_eq!(state.inputs, vec![Input::Microphone]);
        assert_eq!(state.status, Status::Saved);
    }

    #[test]
    fn a_tap_that_stayed_digitally_silent_is_saved_as_microphone_only() {
        // A denied (or unanswered) System Audio Recording permission gives a
        // tap of zeros, just like an allowed tap while nothing plays, and no
        // public API tells them apart. So `inputs` follows what reached the
        // file.
        let dir = TestDir::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let (session, recording) = Session::start(
            dir.path(),
            START,
            Source::Manual,
            Quality::High,
            fake_capture_with(true, 0.0, stopped),
            TICK,
            |_| {},
        )
        .unwrap();
        assert_eq!(recording.computer_audio, ComputerAudio::Recording);
        thread::sleep(Duration::from_millis(100));

        let saved = session.stop(&CopyEncoder::default()).unwrap();

        assert_eq!(saved.inputs, vec![Input::Microphone]);
        let state = read_state(&saved.folder).unwrap();
        assert_eq!(state.inputs, vec![Input::Microphone]);
        assert_eq!(state.status, Status::Saved);
    }

    #[test]
    fn inputs_follow_what_each_source_delivered() {
        let heard = Level {
            peak: 0.001,
            ..Level::default()
        };
        let silent = Level::default();
        let levels = |computer| Levels {
            microphone: heard,
            computer,
        };
        assert_eq!(
            recorded_inputs(&levels(Some(heard))),
            vec![Input::Microphone, Input::ComputerAudio]
        );
        assert_eq!(
            recorded_inputs(&levels(Some(silent))),
            vec![Input::Microphone]
        );
        assert_eq!(recorded_inputs(&levels(None)), vec![Input::Microphone]);
    }

    #[test]
    fn a_file_that_fails_to_close_is_still_saved_and_the_error_returned() {
        let dir = TestDir::new();
        let folder = create_recording_folder(dir.path(), &START).unwrap();
        let mut state = State::new();
        state.inputs = ComputerAudio::Recording.inputs();
        write_state(&folder, &state).unwrap();
        let finished = Finished {
            progress: Progress {
                seconds: 3.0,
                bytes: 288_044,
            },
            levels: Levels {
                microphone: Level::default(),
                computer: Some(Level {
                    peak: 0.5,
                    ..Level::default()
                }),
            },
            dropped: 0,
            error: Some(io::Error::other("disk full")),
        };

        let result = save(&folder, &folder.join("audio.wav"), state, finished, Ok(()));

        assert_eq!(
            result.unwrap_err(),
            RecordingError::Disk("disk full".into())
        );
        let saved = read_state(&folder).unwrap();
        assert_eq!(saved.status, Status::Saved);
        assert_eq!(saved.inputs, vec![Input::Microphone, Input::ComputerAudio]);
    }

    #[test]
    fn if_the_folder_cannot_be_made_the_capture_is_stopped() {
        let dir = TestDir::new();
        let stopped = Arc::new(AtomicBool::new(false));
        let missing = dir.path().join("not-there");
        let result = Session::start(
            &missing,
            START,
            Source::Manual,
            Quality::High,
            fake_capture(true, stopped.clone()),
            TICK,
            |_| {},
        );
        assert!(matches!(result, Err(RecordingError::Disk(_))));
        assert!(stopped.load(Ordering::Acquire));
    }

    #[test]
    fn record_stops_a_running_computer_audio_check_and_waits_for_it() {
        let dir = TestDir::new();
        let recorder = Arc::new(Recorder::new(Arc::new(CopyEncoder::default())));
        let probing = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = mpsc::channel();
        let probe = {
            let (recorder, probing) = (recorder.clone(), probing.clone());
            thread::spawn(move || {
                recorder.probe(|keep_going| {
                    probing.store(true, Ordering::SeqCst);
                    started_tx.send(()).unwrap();
                    let began = Instant::now();
                    while keep_going() && began.elapsed() < Duration::from_secs(5) {
                        thread::sleep(Duration::from_millis(5));
                    }
                    probing.store(false, Ordering::SeqCst);
                    began.elapsed()
                })
            })
        };
        started_rx.recv().unwrap();

        let began = Instant::now();
        recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                high(),
                |_| {
                    // The check's tap and device are gone before the
                    // recording's are made.
                    assert!(!probing.load(Ordering::SeqCst));
                    Ok(fake_capture(true, Arc::new(AtomicBool::new(false))))
                },
                TICK,
                |_| {},
            )
            .unwrap();

        assert!(
            began.elapsed() < Duration::from_secs(1),
            "{:?}",
            began.elapsed()
        );
        // Cut short, so its answer is not used.
        assert_eq!(probe.join().unwrap(), None);
        recorder.stop().unwrap();
    }

    #[test]
    fn no_computer_audio_check_runs_while_recording() {
        let dir = TestDir::new();
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                high(),
                |_| Ok(fake_capture(true, Arc::new(AtomicBool::new(false)))),
                TICK,
                |_| {},
            )
            .unwrap();

        let checked = recorder.probe(|_| -> bool { panic!("must not probe while recording") });

        assert_eq!(checked, None);
        recorder.stop().unwrap();
    }

    #[test]
    fn a_check_that_runs_to_the_end_gives_its_answer() {
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        assert_eq!(recorder.probe(|keep_going| keep_going()), Some(true));
    }

    #[test]
    fn the_recorder_runs_one_recording_at_a_time() {
        let dir = TestDir::new();
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        assert_eq!(recorder.last_computer_audio(), None);
        assert_eq!(recorder.stop().unwrap_err(), RecordingError::NotRecording);

        let flag = || Arc::new(AtomicBool::new(false));
        recorder
            .start(
                dir.path(),
                START,
                Source::Manual,
                high(),
                |_| Ok(fake_capture(false, flag())),
                TICK,
                |_| {},
            )
            .unwrap();
        assert!(recorder.is_recording());
        let second = recorder.start(
            dir.path(),
            START,
            Source::Manual,
            high(),
            |_| panic!("must not start a second capture"),
            TICK,
            |_| {},
        );
        assert_eq!(second.unwrap_err(), RecordingError::AlreadyRecording);
        assert_eq!(
            recorder.last_computer_audio(),
            Some(ComputerAudio::NotAllowed)
        );

        recorder.stop().unwrap();
        assert!(!recorder.is_recording());
    }

    #[test]
    fn a_capture_that_fails_to_start_leaves_no_folder() {
        let dir = TestDir::new();
        let recorder = Recorder::new(Arc::new(CopyEncoder::default()));
        let result = recorder.start(
            dir.path(),
            START,
            Source::Manual,
            high(),
            |_| Err(RecordingError::NoMicrophone),
            TICK,
            |_| {},
        );
        assert_eq!(result.unwrap_err(), RecordingError::NoMicrophone);
        assert!(!recorder.is_recording());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn teardown(started: bool, stopped: bool, destroyed: bool) -> IoTeardown {
        IoTeardown {
            started,
            stopped,
            destroyed,
        }
    }

    #[test]
    fn the_io_context_is_freed_only_when_the_proc_can_no_longer_run() {
        assert!(teardown(true, true, true).may_free_context());
        assert!(teardown(false, false, true).may_free_context());
        // Stop failed: the proc may still be called.
        assert!(!teardown(true, false, true).may_free_context());
        // Still registered.
        assert!(!teardown(true, true, false).may_free_context());
        assert!(!teardown(false, false, false).may_free_context());
    }

    #[test]
    fn the_probe_tone_is_quiet_and_not_silent() {
        let tone = Tone::new(48_000.0);
        let mut out = vec![0.0; 2 * 480];
        tone.fill(&mut out, 2);
        let peak = out.iter().fold(0f32, |peak, s| peak.max(s.abs()));
        assert!(peak > 0.0 && peak <= PROBE_AMPLITUDE, "{peak}");
        // Both channels of a frame carry the same sample.
        assert!(out.chunks(2).all(|frame| frame[0] == frame[1]));
    }

    #[test]
    fn the_probe_tone_continues_across_cycles() {
        let mut tone = Tone::new(48_000.0);
        let mut whole = vec![0.0; 256];
        tone.fill(&mut whole, 1);
        let mut first = vec![0.0; 128];
        let mut second = vec![0.0; 128];
        let mut split = Tone::new(48_000.0);
        split.fill(&mut first, 1);
        split.advance(128);
        split.fill(&mut second, 1);
        for (a, b) in whole[128..].iter().zip(&second) {
            assert!((a - b).abs() < 1e-6);
        }
        tone.advance(256);
    }

    #[test]
    fn only_sound_in_the_tap_buffer_counts() {
        let silence = [0.0f32; 64];
        let mut quiet = [0.0f32; 64];
        quiet[10] = PROBE_AMPLITUDE / 2.0;
        // No sub-device input, then the tap.
        assert!(tap_heard(&[&quiet], 0));
        assert!(!tap_heard(&[&silence], 0));
        // A headset's microphone comes first and does not count.
        assert!(!tap_heard(&[&quiet, &silence], 1));
        assert!(tap_heard(&[&silence, &quiet], 1));
        // No tap buffer at all.
        assert!(!tap_heard(&[&quiet], 1));
        assert!(!tap_heard(&[], 0));
    }

    #[test]
    fn progress_serializes_for_the_interface() {
        let json = serde_json::to_value(Progress {
            seconds: 2.0,
            bytes: 192_044,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({"seconds": 2.0, "bytes": 192_044}));
    }
}
