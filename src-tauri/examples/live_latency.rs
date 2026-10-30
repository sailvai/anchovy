//! `npm run verify:live` runs this: how long after a phrase is finished its
//! words are ready, once the transcription model is loaded.
//!
//! Reads a WAV file and feeds it to the live transcript's phrase finder at
//! the speed it was spoken, 20 ms at a time, each 20 ms only once it would
//! have been heard. Each finished phrase goes to a transcription thread,
//! which loads the transcription model selected in Models when feeding
//! starts, as the app will when Show transcript is pressed. Prints JSON on
//! stdout; `scripts/verify-live.mjs` scores it.
//!
//! Needs the selected transcription model on this Mac (the app's Models
//! screen, or `--fetch`). Never run by `npm run verify`.
//!
//!   cargo run --release --example live_latency -- <clip.wav> [--end-silence S]
//!   cargo run --release --example live_latency -- --fetch

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use anchovy_lib::engines::audio::{read_model_audio, MODEL_RATE};
use anchovy_lib::engines::llama::LlamaEngines;
use anchovy_lib::engines::{mac as memory, Engines, ModelFiles};
use anchovy_lib::live::segmenter::{Params, Segmenter};
use anchovy_lib::live::Phrase;
use anchovy_lib::models::catalog::{Catalog, Role};
use anchovy_lib::models::store::Store;
use anchovy_lib::models::{download, mac, Models};
use anchovy_lib::pipeline::Memory;
use serde::Serialize;

#[derive(Serialize)]
struct Model {
    id: String,
    display_name: String,
    files: Vec<String>,
}

/// One phrase. Times are seconds since feeding started.
#[derive(Serialize)]
struct Heard {
    start_seconds: f64,
    /// Where the phrase finder heard the sound stop.
    voice_end_seconds: f64,
    /// When the phrase finder judged the phrase finished.
    cut_seconds: f64,
    /// When the text was ready to show.
    handed_seconds: f64,
    transcribe_seconds: f64,
    language: Option<String>,
    text: String,
}

#[derive(Serialize)]
struct Output {
    ok: bool,
    error: Option<String>,
    model: Model,
    params: Params,
    audio_seconds: f64,
    model_loaded_seconds: Option<f64>,
    phrases: Vec<Heard>,
    /// How late the latest 20 ms was fed compared with when it was heard.
    max_feed_lag_seconds: f64,
    available_bytes_before: Option<u64>,
    footprint_before: Option<u64>,
    peak_footprint: Option<u64>,
    memory_bytes: u64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--fetch") => fetch(),
        Some(clip) if !clip.starts_with("--") => run(Path::new(clip), &args[1..]),
        _ => Err("usage: live_latency <clip.wav> [--end-silence S] | live_latency --fetch".into()),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn store() -> Result<Store, String> {
    Ok(Store::new(mac::models_dir().map_err(|e| e.to_string())?))
}

/// Downloads the selected transcription model (the shipped default unless
/// another was chosen) through the app's own download code, as
/// `examples/eval.rs --fetch` does.
fn fetch() -> Result<(), String> {
    let catalog = Catalog::shipped();
    let store = store()?;
    let id = store.selected(&catalog, Role::Transcribe);
    let model = catalog.get(&id).ok_or("the selection is in the list")?;
    if store.is_usable(model) {
        eprintln!("{} is on this Mac", model.id);
        return Ok(());
    }
    eprintln!("downloading {}", model.id);
    download::download_model(
        &download::client(),
        &store,
        model,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .map_err(|err| err.to_string())
}

/// Endpoint parameters from the command line, for tuning.
fn params(rest: &[String]) -> Result<Params, String> {
    let mut params = Params::default();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            return Err(format!("{} needs a value", pair[0]));
        };
        let value: f64 = value.parse().map_err(|_| format!("bad {flag}"))?;
        let field = match flag.as_str() {
            "--end-silence" => &mut params.end_silence_seconds,
            "--sound-db" => &mut params.sound_db,
            "--min-sound" => &mut params.min_sound_seconds,
            "--floor" => &mut params.floor_seconds,
            "--pad" => &mut params.pad_seconds,
            _ => return Err(format!("unknown argument {flag}")),
        };
        *field = value;
    }
    Ok(params)
}

fn run(clip: &Path, rest: &[String]) -> Result<(), String> {
    let params = params(rest)?;
    let memory_bytes = mac::memory_bytes().map_err(|err| err.to_string())?;
    let models = Models::new(Catalog::shipped(), store()?, memory_bytes);
    let (files, ready) = models.selected_files(Role::Transcribe);
    if !ready {
        return Err(format!(
            "{} is not downloaded. Run `npm run verify:live -- --fetch` first.",
            files.id
        ));
    }
    let model = Model {
        id: files.id.clone(),
        display_name: files.display_name.clone(),
        files: files
            .files
            .iter()
            .filter_map(|path| path.file_name()?.to_str().map(String::from))
            .collect(),
    };
    let samples = read_model_audio(clip).map_err(|err| err.to_string())?;
    let rate = f64::from(MODEL_RATE);
    let available_bytes_before = memory::MacMemory.available();
    let footprint_before = memory::footprint();

    let started = Instant::now();
    let (phrases_in, phrases_out) = mpsc::channel::<Phrase>();
    let (loaded_in, loaded_out) = mpsc::channel::<f64>();
    let transcription = thread::spawn(move || transcribe(&files, started, phrases_out, loaded_in));

    let mut segmenter = Segmenter::new(params);
    let frame = (params.frame_seconds * rate).round() as usize;
    let mut max_feed_lag_seconds: f64 = 0.0;
    for (index, chunk) in samples.chunks(frame).enumerate() {
        // These 20 ms have been heard only once they are over.
        let heard = Duration::from_secs_f64(((index * frame + chunk.len()) as f64) / rate);
        if let Some(wait) = heard.checked_sub(started.elapsed()) {
            thread::sleep(wait);
        }
        max_feed_lag_seconds =
            max_feed_lag_seconds.max(started.elapsed().as_secs_f64() - heard.as_secs_f64());
        for phrase in segmenter.push(chunk) {
            // A closed channel means loading failed; the thread reports it.
            let _ = phrases_in.send(phrase);
        }
    }
    drop(phrases_in);
    let result = transcription
        .join()
        .map_err(|_| "the transcription thread panicked")?;
    let model_loaded_seconds = loaded_out.try_recv().ok();

    let (ok, error, phrases) = match result {
        Ok(phrases) => (true, None, phrases),
        Err((err, phrases)) => (false, Some(err), phrases),
    };
    let output = Output {
        ok,
        error,
        model,
        params,
        audio_seconds: samples.len() as f64 / rate,
        model_loaded_seconds,
        phrases,
        max_feed_lag_seconds,
        available_bytes_before,
        footprint_before,
        peak_footprint: memory::peak_footprint(),
        memory_bytes,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|err| err.to_string())?
    );
    Ok(())
}

/// Loads the model, then transcribes each phrase in the order it finished.
fn transcribe(
    files: &ModelFiles,
    started: Instant,
    phrases: mpsc::Receiver<Phrase>,
    loaded: mpsc::Sender<f64>,
) -> Result<Vec<Heard>, (String, Vec<Heard>)> {
    let rate = f64::from(MODEL_RATE);
    let mut heard = Vec::new();
    let mut transcriber = LlamaEngines
        .transcriber(files)
        .map_err(|err| (err.to_string(), Vec::new()))?;
    let _ = loaded.send(started.elapsed().as_secs_f64());
    for phrase in phrases {
        let begun = Instant::now();
        let result = transcriber.transcribe(&phrase.samples);
        let transcribe_seconds = begun.elapsed().as_secs_f64();
        let handed_seconds = started.elapsed().as_secs_f64();
        match result {
            Ok(text) => heard.push(Heard {
                start_seconds: phrase.start as f64 / rate,
                voice_end_seconds: phrase.voice_end as f64 / rate,
                cut_seconds: phrase.cut as f64 / rate,
                handed_seconds,
                transcribe_seconds,
                language: text.language,
                text: text.text,
            }),
            Err(err) => return Err((err.to_string(), heard)),
        }
    }
    Ok(heard)
}
