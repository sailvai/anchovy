//! The recording loopback check behind `npm run verify:device` (plan 8.4).
//! Needs a real Mac with audio permissions, so it is an example, not a test.
//!
//!   loopback tone <file.wav> [--seconds 10] [--tone-hz 997]
//!   loopback record [--seconds 5] [--tone-hz 997] [--out <dir>]
//!   loopback probe
//!
//! `record` runs the app's own recorder (the default microphone plus the
//! system-audio tap) while another process plays the tone, then checks that
//! computer audio was captured and that the tone is in the saved file. The
//! tone should start after recording does, so the check also covers a
//! recording that begins while the Mac is silent. It
//! prints a JSON report and exits 1 if a check fails. The microphone needs a
//! person speaking, so it is only reported here, never judged.
//!
//! `probe` runs the one-second computer audio check the first-launch screen
//! uses, and exits 1 unless it reports computer audio as allowed.

use std::f64::consts::PI;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anchovy_lib::notes::note::Source;
use anchovy_lib::notes::state::{read_state, Input, Status};
use anchovy_lib::recording::core::{Progress, Recorder};
use anchovy_lib::recording::file_writer::WavWriter;
use anchovy_lib::recording::mac;
use anchovy_lib::recording::mixer::OUTPUT_RATE;
use serde_json::json;

const TONE_AMPLITUDE: f32 = 0.5;

fn arg<T: std::str::FromStr>(args: &[String], name: &str, default: T) -> T {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn sine(freq: f64, seconds: f64) -> Vec<f32> {
    let n = (OUTPUT_RATE as f64 * seconds) as usize;
    (0..n)
        .map(|i| TONE_AMPLITUDE * (2.0 * PI * freq * i as f64 / OUTPUT_RATE as f64).sin() as f32)
        .collect()
}

/// Amplitude of one frequency (Goertzel). A sine of amplitude A reads about A.
fn tone_amplitude(samples: &[f32], freq: f64) -> f64 {
    let coeff = 2.0 * (2.0 * PI * freq / OUTPUT_RATE as f64).cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    2.0 * power.max(0.0).sqrt() / samples.len().max(1) as f64
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("tone") => tone(&args),
        Some("record") => record(&args),
        Some("probe") => probe(),
        _ => Err(
            "usage: loopback tone <file.wav> | record [--seconds N] [--out <dir>] | probe".into(),
        ),
    };
    match result {
        Ok(true) => {}
        Ok(false) => std::process::exit(1),
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(2);
        }
    }
}

fn probe() -> Result<bool, String> {
    let allowed =
        mac::probe_computer_audio(Duration::from_secs(1), &|| true).map_err(|e| e.to_string())?;
    println!("{}", json!({ "computer_audio_allowed": allowed }));
    Ok(allowed)
}

fn tone(args: &[String]) -> Result<bool, String> {
    let path = args.get(1).ok_or("tone needs a file path")?;
    let seconds = arg(args, "--seconds", 10.0);
    let hz = arg(args, "--tone-hz", 997.0);
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut writer = WavWriter::new(file, OUTPUT_RATE).map_err(|e| e.to_string())?;
    writer
        .write(&sine(hz, seconds))
        .map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(true)
}

fn record(args: &[String]) -> Result<bool, String> {
    let seconds: f64 = arg(args, "--seconds", 5.0);
    let hz: f64 = arg(args, "--tone-hz", 997.0);
    let out: PathBuf = arg(args, "--out", std::env::temp_dir().join("anchovy-loopback"));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;

    let excluded_own_process = mac::own_process_object().is_some();
    let events: Arc<Mutex<Vec<Progress>>> = Arc::default();
    let recorder = Recorder::new();
    let recording = {
        let events = events.clone();
        recorder
            .start(
                &out,
                mac::local_now(),
                Source::Manual,
                || mac::start(None),
                Duration::from_secs(1),
                move |p| events.lock().unwrap().push(p),
            )
            .map_err(|e| e.to_string())?
    };
    std::thread::sleep(Duration::from_secs_f64(seconds));
    let saved = recorder.stop().map_err(|e| e.to_string())?;
    let events = events.lock().unwrap().clone();

    let bytes = std::fs::read(&saved.audio).map_err(|e| e.to_string())?;
    let samples: Vec<f32> = bytes[44..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| i16::from_le_bytes(b) as f32 / i16::MAX as f32)
        .collect();
    let tone_in_file = tone_amplitude(&samples, hz);
    let state = read_state(&saved.folder).map_err(|e| e.to_string())?;
    let computer_rms = saved.levels.computer.map_or(0.0, |l| l.rms);

    let checks = [
        (
            "computer audio was recorded",
            saved.inputs == [Input::Microphone, Input::ComputerAudio],
        ),
        // A 0.5 sine has an RMS of 0.35; -40 dBFS leaves room for the
        // system volume, and is far above the silence of an empty tap.
        ("the computer audio stream has sound", computer_rms > 0.01),
        ("the test tone is in the file", tone_in_file > 0.02),
        (
            "the file is as long as the recording",
            saved.seconds >= seconds - 0.25
                && samples.len() as f64 / OUTPUT_RATE as f64 == saved.seconds,
        ),
        (
            "progress arrived about once a second and grew",
            events.len() as f64 >= seconds.floor() - 1.0
                && events.windows(2).all(|w| w[1].bytes > w[0].bytes),
        ),
        ("the recording is Saved", state.status == Status::Saved),
        ("no audio was dropped", saved.dropped_frames == 0),
    ];
    let passed = checks.iter().all(|(_, ok)| *ok);
    let report = json!({
        "passed": passed,
        "checks": checks.iter().map(|(name, ok)| json!({"check": name, "ok": ok})).collect::<Vec<_>>(),
        "microphone": recording.microphone,
        "computer_audio": recording.computer_audio,
        "excluded_own_process": excluded_own_process,
        "sandboxed": std::env::var("APP_SANDBOX_CONTAINER_ID").is_ok(),
        "audio": saved.audio,
        "seconds": saved.seconds,
        "bytes": saved.bytes,
        "progress_events": events.len(),
        "tone_hz": hz,
        "tone_amplitude_in_file": tone_in_file,
        "levels": saved.levels,
        "note": "The microphone level is reported only. Checking it needs a person speaking (plan 8.9).",
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    Ok(passed)
}
