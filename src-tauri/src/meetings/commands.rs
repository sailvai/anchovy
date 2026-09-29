//! Connects the detector to the system and the interface. A thread reads
//! the process list once a second; a new or vanished prompt is posted or
//! removed as a notification and sent to the window as a `meeting-prompt`
//! event. Record and Not now arrive from either place.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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
        Meetings {
            detector: Mutex::new(Detector::new(std::process::id() as i32, &own_id)),
        }
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
            mac::post(new);
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

/// Record or Not now on prompt `id`. A prompt that has already gone away is
/// left alone and nothing records. Record starts a recording written as
/// `source: meeting`.
fn answer(app: &AppHandle, id: u64, answer: Answer) -> Result<Option<Recording>, String> {
    let answered = app
        .state::<Arc<Meetings>>()
        .detector
        .lock()
        .unwrap()
        .answer(id);
    if !answered {
        return Ok(None);
    }
    mac::remove(id);
    let _ = app.emit(PROMPT_EVENT, None::<Prompt>);
    match answer {
        Answer::Record => recording_commands::start(app, Source::Meeting, None).map(Some),
        Answer::NotNow => Ok(None),
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
