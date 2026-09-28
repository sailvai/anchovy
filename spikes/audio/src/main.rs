//! Plan step 4a spike. Records from the selected microphone plus a global
//! system-audio tap, both in one private aggregate device, and writes every
//! channel to one WAV file with a JSON report next to it.
//!
//!   audio-spike list
//!   audio-spike record [--device <name part>] [--seconds 10] [--out <dir>] [--tone-hz 1000]
//!   audio-spike tone <file.wav> [--seconds 12] [--tone-hz 1000]
//!
//! `--out` defaults to the temporary directory, which inside the App Sandbox is
//! the app's container.

mod mac;

use std::path::PathBuf;
use std::time::Duration;

use audio_spike::{Stream, interleave, measure, report_json, sine, wav_f32};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("list") => list(),
        Some("record") => record(&args),
        Some("tone") => tone(&args),
        _ => Err("usage: audio-spike list | record [--device <name>] [--seconds N] [--out <dir>] | tone <file>".into()),
    };
    if let Err(message) = result {
        eprintln!("error: {message}");
        std::process::exit(1);
    }
}

fn list() -> mac::Result<()> {
    let default = mac::default_input_device()?;
    for d in mac::input_devices()? {
        let mark = if d.id == default { "*" } else { " " };
        println!(
            "{mark} {:>4}  {} ch  {}  ({})",
            d.id, d.input_channels, d.name, d.uid
        );
    }
    Ok(())
}

fn tone(args: &[String]) -> mac::Result<()> {
    let path = args.get(1).ok_or("tone needs a file path")?;
    let seconds: f64 = arg(args, "--seconds").map_or(12.0, |s| s.parse().unwrap());
    let hz: f64 = arg(args, "--tone-hz").map_or(1000.0, |s| s.parse().unwrap());
    std::fs::write(path, wav_f32(1, 48_000, &sine(hz, 0.5, 48_000, seconds)))
        .map_err(|e| e.to_string())
}

fn record(args: &[String]) -> mac::Result<()> {
    let seconds: u64 = arg(args, "--seconds").map_or(10, |s| s.parse().unwrap());
    let tone_hz: f64 = arg(args, "--tone-hz").map_or(1000.0, |s| s.parse().unwrap());
    let out_dir = arg(args, "--out").map_or_else(std::env::temp_dir, PathBuf::from);

    let devices = mac::input_devices()?;
    let default = mac::default_input_device()?;
    let mic = match arg(args, "--device") {
        Some(part) => devices.iter().find(|d| d.name.contains(&part)),
        None => devices.iter().find(|d| d.id == default),
    }
    .ok_or("microphone not found")?;
    let mic_streams = mac::input_streams(mic.id)?;
    eprintln!(
        "microphone: {} ({} input streams {:?})",
        mic.name,
        mic_streams.len(),
        mic_streams
    );

    let tap = mac::create_global_tap()?;
    eprintln!(
        "tap: id {} uid {} format {} Hz, {} ch",
        tap.id, tap.uid, tap.format.mSampleRate, tap.format.mChannelsPerFrame
    );
    let outcome = (|| {
        let aggregate = mac::create_aggregate(&mic.uid, &tap.uid)?;
        let run = (|| {
            let rate = mac::nominal_sample_rate(aggregate)?;
            let layout = mac::input_streams(aggregate)?;
            eprintln!("aggregate: id {aggregate}, {rate} Hz, input streams {layout:?}");
            // The main sub-device's streams come first, then the tap.
            let streams: Vec<Stream> = layout
                .iter()
                .enumerate()
                .map(|(i, &ch)| {
                    Stream::new(
                        if i < mic_streams.len() {
                            "microphone"
                        } else {
                            "system"
                        },
                        ch,
                    )
                })
                .collect();
            eprintln!("recording {seconds} s ...");
            let streams = mac::record(aggregate, streams, Duration::from_secs(seconds))?;
            Ok::<_, String>((rate, layout, streams))
        })();
        mac::destroy_aggregate(aggregate)?;
        run
    })();
    mac::destroy_tap(tap.id)?;
    let (rate, layout, streams) = outcome?;

    let (channels, samples) = interleave(&streams);
    let wav = out_dir.join("audio-spike.wav");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    std::fs::write(&wav, wav_f32(channels, rate as u32, &samples))
        .map_err(|e| format!("{}: {e}", wav.display()))?;
    let rows = measure(&streams, tone_hz, rate as u32);
    let fields = [
        ("microphone", mic.name.clone()),
        ("sample_rate", rate.to_string()),
        ("aggregate_input_streams", format!("{layout:?}")),
        (
            "frames",
            streams
                .iter()
                .map(Stream::frames)
                .max()
                .unwrap_or(0)
                .to_string(),
        ),
        ("tone_hz", tone_hz.to_string()),
        (
            "sandboxed",
            std::env::var("APP_SANDBOX_CONTAINER_ID")
                .is_ok()
                .to_string(),
        ),
        ("wav", wav.display().to_string()),
    ];
    let report = report_json(&fields, &rows);
    std::fs::write(out_dir.join("audio-spike.json"), &report).map_err(|e| e.to_string())?;
    print!("{report}");
    Ok(())
}
