//! Connects the detector to the system and the interface. A thread reads
//! the process list once a second; a new or vanished prompt is posted or
//! removed as a notification and sent to the window as a `meeting-prompt`
//! event. Record and Not now arrive from either place.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};

use super::detector::{Detector, Prompt};
use super::mac::{self, Choice};
use crate::library::Library;
use crate::notes::note::Source;
use crate::recording::commands as recording_commands;
use crate::recording::core::{Recorder, Recording};
use crate::setup::can_record;

pub const PROMPT_EVENT: &str = "meeting-prompt";
/// A Record from the notification that could not start. The window shows
/// the reason.
pub const RECORD_FAILED_EVENT: &str = "meeting-record-failed";
const POLL: Duration = Duration::from_secs(1);

pub struct Meetings {
    detector: Mutex<Detector>,
}

impl Meetings {
    pub fn new() -> Self {
        let own_id = mac::own_bundle_id().unwrap_or_else(|| "com.sailvai.anchovy".into());
        // Prompt IDs start at this launch's time in milliseconds, so an
        // answer from an earlier launch's notification matches nothing.
        let first_id = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(1, |since| since.as_millis() as u64);
        Meetings {
            detector: Mutex::new(Detector::new(std::process::id() as i32, &own_id, first_id)),
        }
    }

    fn is_showing(&self, id: u64) -> bool {
        self.detector
            .lock()
            .unwrap()
            .prompt()
            .is_some_and(|p| p.id == id)
    }
}

impl Default for Meetings {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    Record,
    NotNow,
}

/// Starts watching for meetings and handling the notification's buttons.
pub fn watch(app: AppHandle) {
    let from_notification = app.clone();
    mac::install(move |id, choice| {
        let app = from_notification.clone();
        // Starting a recording takes a moment; the notification center's
        // thread should not wait for it.
        thread::spawn(move || match choice {
            Choice::Record => {
                show_window(&app);
                if let Err(reason) = answer(&app, id, Answer::Record) {
                    let _ = app.emit(RECORD_FAILED_EVENT, reason);
                }
            }
            Choice::NotNow => {
                let _ = answer(&app, id, Answer::NotNow);
            }
            Choice::Open => show_window(&app),
        });
    });
    thread::spawn(move || loop {
        check(&app);
        thread::sleep(POLL);
    });
}

fn check(app: &AppHandle) {
    let processes = mac::processes();
    let may_ask = !app.state::<Arc<Recorder>>().is_recording()
        && can_record(
            app.state::<Library>().notes_dir().is_some(),
            crate::setup::mac::microphone_access(),
        );
    let meetings = app.state::<Arc<Meetings>>();
    let (before, after) = {
        let mut detector = meetings.detector.lock().unwrap();
        let before = detector.prompt().cloned();
        let after = detector
            .observe(&processes, may_ask, Instant::now())
            .cloned();
        (before, after)
    };
    if before != after {
        if let Some(gone) = &before {
            mac::remove(gone.id);
        }
        if let Some(new) = &after {
            let meetings = meetings.inner().clone();
            let id = new.id;
            mac::post(new, move || meetings.is_showing(id));
        }
        let _ = app.emit(PROMPT_EVENT, &after);
    }
}

fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Record or Not now on prompt `id`, from the banner or the notification.
fn answer(app: &AppHandle, id: u64, answer: Answer) -> Result<Option<Recording>, String> {
    resolve(
        &app.state::<Arc<Meetings>>().detector,
        id,
        answer,
        app.state::<Arc<Recorder>>().is_recording(),
        || {
            mac::remove(id);
            let _ = app.emit(PROMPT_EVENT, None::<Prompt>);
        },
        || recording_commands::start(app, Source::Meeting, None),
    )
}

/// What an answer does, with the system calls passed in. A prompt that has
/// already gone away is left alone and nothing records. Otherwise the
/// prompt is taken away (`gone`), and Record starts a recording written as
/// `source: meeting`, unless one is already running: then the meeting is
/// being recorded and the answer only closes the prompt.
fn resolve<T>(
    detector: &Mutex<Detector>,
    id: u64,
    answer: Answer,
    recording: bool,
    gone: impl FnOnce(),
    start: impl FnOnce() -> Result<T, String>,
) -> Result<Option<T>, String> {
    if !detector.lock().unwrap().answer(id) {
        return Ok(None);
    }
    gone();
    match answer {
        Answer::Record if !recording => start().map(Some),
        Answer::Record | Answer::NotNow => Ok(None),
    }
}

/// The prompt showing now, for a window that has just opened.
#[tauri::command]
pub fn meeting_prompt(meetings: State<Arc<Meetings>>) -> Option<Prompt> {
    meetings.detector.lock().unwrap().prompt().cloned()
}

/// The banner's Record and Not now.
#[tauri::command]
pub fn answer_meeting_prompt(
    app: AppHandle,
    id: u64,
    answer: Answer,
) -> Result<Option<Recording>, String> {
    self::answer(&app, id, answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::detector::Process;
    use std::cell::Cell;

    /// A detector showing a Zoom prompt, and its ID.
    fn prompting() -> (Mutex<Detector>, u64) {
        let mut detector = Detector::new(1, "com.sailvai.anchovy", 7);
        let zoom = [Process {
            pid: 20,
            bundle_id: "us.zoom.xos".into(),
            using_microphone: true,
        }];
        let start = Instant::now();
        for second in 0..=5 {
            detector.observe(&zoom, true, start + Duration::from_secs(second));
        }
        let id = detector.prompt().expect("a prompt").id;
        (Mutex::new(detector), id)
    }

    /// Runs `resolve` and counts what it did.
    fn run(
        detector: &Mutex<Detector>,
        id: u64,
        answer: Answer,
        recording: bool,
        started: Result<&'static str, String>,
    ) -> (Result<Option<&'static str>, String>, u32, u32) {
        let (gone, starts) = (Cell::new(0), Cell::new(0));
        let result = resolve(
            detector,
            id,
            answer,
            recording,
            || gone.set(gone.get() + 1),
            || {
                starts.set(starts.get() + 1);
                started
            },
        );
        (result, gone.get(), starts.get())
    }

    #[test]
    fn record_closes_the_prompt_and_starts_one_recording() {
        let (detector, id) = prompting();
        let (result, gone, starts) = run(&detector, id, Answer::Record, false, Ok("recording"));
        assert_eq!(result, Ok(Some("recording")));
        assert_eq!((gone, starts), (1, 1));
        assert_eq!(detector.lock().unwrap().prompt(), None);
    }

    #[test]
    fn not_now_closes_the_prompt_and_records_nothing() {
        let (detector, id) = prompting();
        let (result, gone, starts) = run(&detector, id, Answer::NotNow, false, Ok("recording"));
        assert_eq!(result, Ok(None));
        assert_eq!((gone, starts), (1, 0));
        assert_eq!(detector.lock().unwrap().prompt(), None);
    }

    #[test]
    fn an_answer_to_a_prompt_that_has_gone_does_nothing() {
        let (detector, id) = prompting();
        for answer in [Answer::Record, Answer::NotNow] {
            let (result, gone, starts) = run(&detector, id + 1, answer, false, Ok("recording"));
            assert_eq!(result, Ok(None));
            assert_eq!((gone, starts), (0, 0));
        }
        assert_eq!(detector.lock().unwrap().prompt().map(|p| p.id), Some(id));
    }

    #[test]
    fn record_while_already_recording_only_closes_the_prompt() {
        let (detector, id) = prompting();
        let (result, gone, starts) = run(&detector, id, Answer::Record, true, Ok("recording"));
        assert_eq!(result, Ok(None));
        assert_eq!((gone, starts), (1, 0));
    }

    #[test]
    fn a_recording_that_cannot_start_says_why() {
        let (detector, id) = prompting();
        let why = "Anchovy needs microphone access to record.".to_string();
        let (result, gone, _) = run(&detector, id, Answer::Record, false, Err(why.clone()));
        assert_eq!(result, Err(why));
        assert_eq!(gone, 1);
    }
}
