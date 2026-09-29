//! `npm run eval` runs this once per sample: the app's own pipeline, with the
//! shipped default models, on one WAV clip. It prints what the pipeline
//! produced and what it cost as JSON on stdout; `scripts/eval.mjs` scores it.
//!
//! Needs the default models downloaded on this Mac (the app's Models screen,
//! or `--fetch`). Never run by `npm run verify`.
//!
//!   cargo run --release --example eval -- <clip.wav> [--chunk-tokens N]
//!   cargo run --release --example eval -- --fetch

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use anchovy_lib::engines::llama::LlamaEngines;
use anchovy_lib::engines::{
    mac as memory, summary, EngineError, Engines, Heard, ModelFiles, Prompt, Summarizer,
    Transcriber,
};
use anchovy_lib::models::catalog::{Catalog, Role};
use anchovy_lib::models::store::Store;
use anchovy_lib::models::{download, mac};
use anchovy_lib::notes::folder::{create_recording_folder, StartTime};
use anchovy_lib::notes::state::{write_state, Input, State, Status};
use anchovy_lib::pipeline::{self, Deps, ModelSource};
use serde::Serialize;

/// The shipped defaults, whatever is selected in the app.
struct Defaults {
    catalog: Catalog,
    store: Store,
}

impl ModelSource for Defaults {
    fn selected(&self, role: Role) -> (ModelFiles, bool) {
        let model = self.catalog.default_for(role);
        let dir = self.store.model_dir(model);
        let files = ModelFiles {
            id: model.id.clone(),
            engine: model.engine.clone(),
            display_name: model.display_name.clone(),
            files: model.files.iter().map(|f| dir.join(&f.name)).collect(),
            size_bytes: model.size(),
        };
        (files, self.store.is_usable(model))
    }
}

#[derive(Serialize, Clone)]
struct Answer {
    attempt: u32,
    text: String,
}

/// Wraps the real engines to keep every answer the summary model gave.
struct Recording {
    inner: LlamaEngines,
    answers: Arc<std::sync::Mutex<Vec<Answer>>>,
    heard: Arc<std::sync::Mutex<Vec<Heard>>>,
}

/// Keeps what the speech model heard in each window, so a failed run still
/// shows its transcript.
struct RecordingTranscriber {
    inner: Box<dyn Transcriber>,
    heard: Arc<std::sync::Mutex<Vec<Heard>>>,
}

impl Transcriber for RecordingTranscriber {
    fn transcribe(&mut self, samples: &[f32]) -> Result<Heard, EngineError> {
        let heard = self.inner.transcribe(samples)?;
        self.heard.lock().unwrap().push(heard.clone());
        Ok(heard)
    }
}

struct RecordingSummarizer {
    inner: Box<dyn Summarizer>,
    answers: Arc<std::sync::Mutex<Vec<Answer>>>,
}

impl Summarizer for RecordingSummarizer {
    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        self.inner.count_tokens(text)
    }

    fn complete(&mut self, prompt: &Prompt, attempt: u32) -> Result<String, EngineError> {
        let text = self.inner.complete(prompt, attempt)?;
        self.answers.lock().unwrap().push(Answer {
            attempt,
            text: text.clone(),
        });
        Ok(text)
    }
}

impl Engines for Recording {
    fn transcriber(&self, model: &ModelFiles) -> Result<Box<dyn Transcriber>, EngineError> {
        Ok(Box::new(RecordingTranscriber {
            inner: self.inner.transcriber(model)?,
            heard: self.heard.clone(),
        }))
    }

    fn summarizer(
        &self,
        model: &ModelFiles,
        context_tokens: u32,
    ) -> Result<Box<dyn Summarizer>, EngineError> {
        Ok(Box::new(RecordingSummarizer {
            inner: self.inner.summarizer(model, context_tokens)?,
            answers: self.answers.clone(),
        }))
    }

    fn summarizer_bytes(&self, model: &ModelFiles, context_tokens: u32) -> u64 {
        self.inner.summarizer_bytes(model, context_tokens)
    }
}

#[derive(Serialize)]
struct Output {
    ok: bool,
    error: Option<String>,
    report: Option<pipeline::Report>,
    note: Option<String>,
    answers: Vec<Answer>,
    /// Each window's text before the joins, with the language reported.
    heard: Vec<(Option<String>, String)>,
    chunk_tokens: usize,
    wall_seconds: f64,
    peak_footprint: Option<u64>,
    memory_bytes: u64,
    stages: Vec<pipeline::Stage>,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--fetch") => fetch(),
        Some(clip) if !clip.starts_with("--") => run(Path::new(clip), &args[1..]),
        _ => Err("usage: eval <clip.wav> [--chunk-tokens N] | eval --fetch".into()),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn defaults() -> Result<Defaults, String> {
    Ok(Defaults {
        catalog: Catalog::shipped(),
        store: Store::new(mac::models_dir().map_err(|err| err.to_string())?),
    })
}

/// Downloads the default models through the app's own download code.
fn fetch() -> Result<(), String> {
    let defaults = defaults()?;
    let client = download::client();
    for role in Role::ALL {
        let model = defaults.catalog.default_for(role);
        if defaults.store.is_usable(model) {
            eprintln!("{} is on this Mac", model.id);
            continue;
        }
        eprintln!("downloading {}", model.id);
        download::download_model(
            &client,
            &defaults.store,
            model,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn run(clip: &Path, rest: &[String]) -> Result<(), String> {
    let memory_bytes = mac::memory_bytes().map_err(|err| err.to_string())?;
    let chunk_tokens = match rest {
        [] => summary::chunk_tokens(memory_bytes),
        [flag, n] if flag == "--chunk-tokens" => n.parse().map_err(|_| "bad --chunk-tokens")?,
        _ => return Err("unknown arguments".into()),
    };
    let defaults = defaults()?;
    for role in Role::ALL {
        let (model, ready) = defaults.selected(role);
        if !ready {
            return Err(format!(
                "{} is not downloaded. Run `npm run eval -- --fetch` first.",
                model.id
            ));
        }
    }

    // A recording folder like the app makes, in a scratch notes folder.
    let notes_dir = std::env::temp_dir().join(format!("anchovy-eval-{}", std::process::id()));
    let _ = fs::remove_dir_all(&notes_dir);
    fs::create_dir_all(&notes_dir).map_err(|err| err.to_string())?;
    let start = StartTime {
        year: 2026,
        month: 1,
        day: 1,
        hour: 9,
        minute: 0,
        second: 0,
    };
    let folder: PathBuf = create_recording_folder(&notes_dir, &start).map_err(|e| e.to_string())?;
    fs::copy(clip, folder.join("audio.wav")).map_err(|err| format!("{}: {err}", clip.display()))?;
    let mut state = State::new();
    state.inputs = vec![Input::Microphone];
    state
        .move_to(Status::Saved)
        .map_err(|err| err.to_string())?;
    write_state(&folder, &state).map_err(|err| err.to_string())?;

    let answers = Arc::new(std::sync::Mutex::new(Vec::new()));
    let heard = Arc::new(std::sync::Mutex::new(Vec::new()));
    let engines = Arc::new(Recording {
        inner: LlamaEngines,
        answers: answers.clone(),
        heard: heard.clone(),
    });
    let deps = Deps {
        models: Arc::new(defaults),
        engines,
        memory: Arc::new(memory::MacMemory),
        chunk_tokens,
        memory_wait: pipeline::MEMORY_WAIT,
    };
    let stages = Rc::new(RefCell::new(Vec::new()));
    let seen = stages.clone();
    let started = Instant::now();
    let result = pipeline::run(&folder, &deps, &mut |stage| {
        eprintln!("{stage:?}");
        seen.borrow_mut().push(stage);
    });
    let wall_seconds = started.elapsed().as_secs_f64();
    let note = fs::read_to_string(folder.join("note.md")).ok();
    let _ = fs::remove_dir_all(&notes_dir);

    let (ok, error, report) = match result {
        Ok(report) => (true, None, Some(report)),
        Err(err) => (false, Some(err), None),
    };
    let output = Output {
        ok,
        error,
        report,
        note,
        answers: answers.lock().unwrap().clone(),
        heard: heard
            .lock()
            .unwrap()
            .iter()
            .map(|h| (h.language.clone(), h.text.clone()))
            .collect(),
        chunk_tokens,
        wall_seconds,
        peak_footprint: memory::peak_footprint(),
        memory_bytes,
        stages: stages.borrow().clone(),
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|err| err.to_string())?
    );
    Ok(())
}
