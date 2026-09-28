//! Plan step 4a spike: the testable half. Collects the aggregate device's input
//! buffers per stream, writes them to one float WAV file, and measures each
//! stream so a script can tell whether the microphone and the system-audio tap
//! both reached the file. Core Audio calls live in `mac.rs`.
//!
//! Not the real recorder: no resampling, no mixing, no limiter.

use std::f64::consts::PI;

/// One input stream of the aggregate device, as delivered in the IO callback.
#[derive(Debug, Clone, PartialEq)]
pub struct Stream {
    pub label: String,
    pub channels: usize,
    /// Interleaved samples, `channels` per frame.
    pub samples: Vec<f32>,
}

impl Stream {
    pub fn new(label: &str, channels: usize) -> Self {
        Stream {
            label: label.to_string(),
            channels,
            samples: Vec::new(),
        }
    }

    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }

    /// Channel `index` as its own sample vector.
    pub fn channel(&self, index: usize) -> Vec<f32> {
        self.samples
            .iter()
            .skip(index)
            .step_by(self.channels)
            .copied()
            .collect()
    }
}

/// Appends one IO cycle. `buffers` are the callback's buffers in order, one per
/// stream. Buffers beyond the known streams are ignored.
pub fn append_cycle(streams: &mut [Stream], buffers: &[&[f32]]) {
    for (stream, buffer) in streams.iter_mut().zip(buffers) {
        stream.samples.extend_from_slice(buffer);
    }
}

/// All streams side by side in one interleaved frame sequence, mic channels
/// first. Shorter streams are padded with silence.
pub fn interleave(streams: &[Stream]) -> (usize, Vec<f32>) {
    let channels: usize = streams.iter().map(|s| s.channels).sum();
    let frames = streams.iter().map(Stream::frames).max().unwrap_or(0);
    let mut out = Vec::with_capacity(frames * channels);
    for frame in 0..frames {
        for stream in streams {
            for ch in 0..stream.channels {
                out.push(
                    stream
                        .samples
                        .get(frame * stream.channels + ch)
                        .copied()
                        .unwrap_or(0.0),
                );
            }
        }
    }
    (channels, out)
}

/// A 32-bit float WAV file (WAVE_FORMAT_IEEE_FLOAT).
pub fn wav_f32(channels: usize, sample_rate: u32, interleaved: &[f32]) -> Vec<u8> {
    let data_len = (interleaved.len() * 4) as u32;
    let block_align = (channels * 4) as u16;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&3u16.to_le_bytes());
    out.extend_from_slice(&(channels as u16).to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in interleaved {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

/// A mono sine wave, used as the known sound played through the speakers.
pub fn sine(freq: f64, amplitude: f32, sample_rate: u32, seconds: f64) -> Vec<f32> {
    let n = (sample_rate as f64 * seconds) as usize;
    (0..n)
        .map(|i| amplitude * (2.0 * PI * freq * i as f64 / sample_rate as f64).sin() as f32)
        .collect()
}

/// RMS level in dBFS. Digital silence reports -inf.
pub fn rms_dbfs(samples: &[f32]) -> f64 {
    if samples.is_empty() {
        return f64::NEG_INFINITY;
    }
    let mean = samples
        .iter()
        .map(|&s| (s as f64) * (s as f64))
        .sum::<f64>()
        / samples.len() as f64;
    10.0 * mean.log10()
}

/// Level of one frequency in dBFS (a sine of amplitude A reads about
/// 20*log10(A/sqrt 2)), via the Goertzel algorithm.
pub fn tone_dbfs(samples: &[f32], freq: f64, sample_rate: u32) -> f64 {
    if samples.is_empty() {
        return f64::NEG_INFINITY;
    }
    let w = 2.0 * PI * freq / sample_rate as f64;
    let coeff = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &x in samples {
        let s0 = x as f64 + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
    let amplitude = 2.0 * power.sqrt() / samples.len() as f64;
    20.0 * (amplitude / 2f64.sqrt()).log10()
}

/// Per-channel measurements for the report.
#[derive(Debug, Clone, PartialEq)]
pub struct ChannelReport {
    pub stream: String,
    pub channel: usize,
    pub rms_dbfs: f64,
    pub tone_dbfs: f64,
    pub all_zero: bool,
}

pub fn measure(streams: &[Stream], tone_hz: f64, sample_rate: u32) -> Vec<ChannelReport> {
    let mut out = Vec::new();
    for stream in streams {
        for ch in 0..stream.channels {
            let samples = stream.channel(ch);
            out.push(ChannelReport {
                stream: stream.label.clone(),
                channel: ch,
                rms_dbfs: rms_dbfs(&samples),
                tone_dbfs: tone_dbfs(&samples, tone_hz, sample_rate),
                all_zero: samples.iter().all(|&s| s == 0.0),
            });
        }
    }
    out
}

/// How one recording's IO proc ended. `stopped` is only meaningful when
/// `started` is true.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IoTeardown {
    pub started: bool,
    pub stopped: bool,
    pub destroyed: bool,
}

/// Whether the IO proc's context may be freed. Core Audio keeps calling a
/// registered, running IO proc with that pointer, so it must outlive the proc.
pub fn may_free_io_context(teardown: IoTeardown) -> bool {
    let not_running = !teardown.started || teardown.stopped;
    not_running && teardown.destroyed
}

/// Hand-written JSON so the spike needs no serde.
pub fn report_json(fields: &[(&str, String)], channels: &[ChannelReport]) -> String {
    let num = |v: f64| {
        if v.is_finite() {
            format!("{v:.1}")
        } else {
            "null".to_string()
        }
    };
    let mut out = String::from("{\n");
    for (key, value) in fields {
        out.push_str(&format!(
            "  \"{key}\": \"{}\",\n",
            value.replace('\\', "\\\\").replace('"', "\\\"")
        ));
    }
    out.push_str("  \"channels\": [\n");
    let rows: Vec<String> = channels
        .iter()
        .map(|c| {
            format!(
                "    {{\"stream\": \"{}\", \"channel\": {}, \"rms_dbfs\": {}, \"tone_dbfs\": {}, \"all_zero\": {}}}",
                c.stream, c.channel, num(c.rms_dbfs), num(c.tone_dbfs), c.all_zero
            )
        })
        .collect();
    out.push_str(&rows.join(",\n"));
    out.push_str("\n  ]\n}\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_cycle_keeps_streams_apart() {
        let mut streams = vec![Stream::new("microphone", 1), Stream::new("system", 2)];
        append_cycle(&mut streams, &[&[0.1, 0.2], &[1.0, -1.0, 0.5, -0.5]]);
        append_cycle(&mut streams, &[&[0.3], &[0.25, -0.25]]);
        assert_eq!(streams[0].samples, vec![0.1, 0.2, 0.3]);
        assert_eq!(streams[1].frames(), 3);
        assert_eq!(streams[1].channel(1), vec![-1.0, -0.5, -0.25]);
    }

    #[test]
    fn append_cycle_ignores_unknown_extra_buffers() {
        let mut streams = vec![Stream::new("microphone", 1)];
        append_cycle(&mut streams, &[&[0.1], &[9.0]]);
        assert_eq!(streams[0].samples, vec![0.1]);
    }

    #[test]
    fn interleave_puts_mic_first_and_pads_short_streams() {
        let mut mic = Stream::new("microphone", 1);
        mic.samples = vec![0.1, 0.2];
        let mut sys = Stream::new("system", 2);
        sys.samples = vec![1.0, 2.0];
        let (channels, out) = interleave(&[mic, sys]);
        assert_eq!(channels, 3);
        assert_eq!(out, vec![0.1, 1.0, 2.0, 0.2, 0.0, 0.0]);
    }

    #[test]
    fn wav_header_describes_float_samples() {
        let bytes = wav_f32(3, 48_000, &[0.5; 6]);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 36 + 24);
        assert_eq!(u16::from_le_bytes(bytes[20..22].try_into().unwrap()), 3);
        assert_eq!(u16::from_le_bytes(bytes[22..24].try_into().unwrap()), 3);
        assert_eq!(
            u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            48_000
        );
        assert_eq!(
            u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            48_000 * 12
        );
        assert_eq!(u16::from_le_bytes(bytes[32..34].try_into().unwrap()), 12);
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 24);
        assert_eq!(bytes.len(), 44 + 24);
        assert_eq!(f32::from_le_bytes(bytes[44..48].try_into().unwrap()), 0.5);
    }

    #[test]
    fn silence_is_minus_infinity_and_full_scale_sine_is_about_minus_3() {
        assert_eq!(rms_dbfs(&[0.0; 100]), f64::NEG_INFINITY);
        let s = sine(1000.0, 1.0, 48_000, 1.0);
        assert!((rms_dbfs(&s) + 3.01).abs() < 0.05);
    }

    #[test]
    fn tone_is_found_at_its_frequency_and_not_elsewhere() {
        let s = sine(1000.0, 0.5, 48_000, 1.0);
        let at = tone_dbfs(&s, 1000.0, 48_000);
        assert!((at - (-9.03)).abs() < 0.1, "{at}");
        assert!(tone_dbfs(&s, 440.0, 48_000) < -60.0);
    }

    #[test]
    fn measure_reports_every_channel_and_flags_digital_silence() {
        let mut sys = Stream::new("system", 2);
        sys.samples = sine(1000.0, 0.5, 48_000, 0.5)
            .iter()
            .flat_map(|&x| [x, 0.0])
            .collect();
        let rows = measure(&[sys], 1000.0, 48_000);
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].all_zero && rows[0].tone_dbfs > -10.0);
        assert!(rows[1].all_zero && rows[1].rms_dbfs == f64::NEG_INFINITY);
    }

    #[test]
    fn report_json_escapes_strings_and_writes_null_for_silence() {
        let rows = vec![ChannelReport {
            stream: "system".into(),
            channel: 0,
            rms_dbfs: f64::NEG_INFINITY,
            tone_dbfs: -20.04,
            all_zero: true,
        }];
        let json = report_json(&[("device", "Mic \"A\"".into())], &rows);
        assert!(json.contains(r#""device": "Mic \"A\"""#));
        assert!(json.contains(r#""rms_dbfs": null, "tone_dbfs": -20.0, "all_zero": true"#));
    }

    fn teardown(started: bool, stopped: bool, destroyed: bool) -> IoTeardown {
        IoTeardown {
            started,
            stopped,
            destroyed,
        }
    }

    #[test]
    fn io_context_is_freed_after_a_clean_teardown() {
        assert!(may_free_io_context(teardown(true, true, true)));
        assert!(may_free_io_context(teardown(false, false, true)));
    }

    #[test]
    fn io_context_is_kept_when_stop_fails_because_the_proc_may_still_run() {
        assert!(!may_free_io_context(teardown(true, false, true)));
        assert!(!may_free_io_context(teardown(true, false, false)));
    }

    #[test]
    fn io_context_is_kept_when_the_proc_is_still_registered() {
        assert!(!may_free_io_context(teardown(true, true, false)));
        assert!(!may_free_io_context(teardown(false, false, false)));
    }
}
