//! Turns the two captured streams into the one mono track that is written to
//! disk: each stream is resampled to 48 kHz, the two are summed, and a limiter
//! keeps the sum from clipping. No system calls, so everything here is tested
//! with synthetic signals.

/// Every recording is written at this rate, whatever the microphone runs at.
pub const OUTPUT_RATE: u32 = 48_000;

/// The limiter keeps every output sample at or below this level (-0.5 dBFS).
pub const CEILING: f32 = 0.944;

/// How long the limiter takes to recover most of its gain after a peak.
const RELEASE_SECONDS: f32 = 0.2;

/// Streaming linear-interpolation resampler for one mono stream. Feeding the
/// input in chunks gives the same output as feeding it all at once.
///
/// Linear interpolation is enough for speech that is later resampled again
/// for transcription. When the rates match, samples pass through unchanged.
pub struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Position of the next output sample, in input samples, relative to the
    /// start of the next chunk. -1 is the last sample of the previous chunk.
    position: f64,
    last: Option<f32>,
}

impl Resampler {
    pub fn new(input_rate: f64, output_rate: f64) -> Self {
        Resampler {
            step: input_rate / output_rate,
            position: 0.0,
            last: None,
        }
    }

    /// Appends the resampled `input` to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.step == 1.0 {
            out.extend_from_slice(input);
            return;
        }
        let Some(&final_sample) = input.last() else {
            return;
        };
        let last = self.last;
        let at = |i: isize| {
            if i < 0 {
                last.unwrap_or(input[0])
            } else {
                input[i as usize]
            }
        };
        let end = (input.len() - 1) as f64;
        while self.position <= end {
            let index = self.position.floor();
            let frac = (self.position - index) as f32;
            let a = at(index as isize);
            let sample = if frac == 0.0 {
                a
            } else {
                a + (at(index as isize + 1) - a) * frac
            };
            out.push(sample);
            self.position += self.step;
        }
        self.position -= input.len() as f64;
        self.last = Some(final_sample);
    }
}

/// Peak limiter with instant attack: a sample that would exceed `CEILING`
/// pulls the gain down at once, then the gain recovers smoothly.
pub struct Limiter {
    gain: f32,
    /// Per-sample share of the distance back to unity gain.
    release: f32,
}

impl Limiter {
    pub fn new(sample_rate: u32) -> Self {
        Limiter {
            gain: 1.0,
            release: 1.0 - (-1.0 / (RELEASE_SECONDS * sample_rate as f32)).exp(),
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        for sample in samples {
            if !sample.is_finite() {
                *sample = 0.0;
            }
            let level = sample.abs();
            let needed = if level > CEILING {
                CEILING / level
            } else {
                1.0
            };
            let recovered = self.gain + (1.0 - self.gain) * self.release;
            self.gain = recovered.min(needed);
            // The clamp only catches float rounding in `CEILING / level`.
            *sample = (*sample * self.gain).clamp(-CEILING, CEILING);
        }
    }
}

/// Resamples the microphone and, when it is recorded, computer audio, then
/// sums and limits them. Both streams come from one aggregate device, so they
/// arrive at the same rate and in chunks of the same length.
pub struct Mixer {
    microphone: Resampler,
    computer: Option<Resampler>,
    limiter: Limiter,
    computer_out: Vec<f32>,
}

impl Mixer {
    pub fn new(input_rate: f64, computer_audio: bool) -> Self {
        let output_rate = OUTPUT_RATE as f64;
        Mixer {
            microphone: Resampler::new(input_rate, output_rate),
            computer: computer_audio.then(|| Resampler::new(input_rate, output_rate)),
            limiter: Limiter::new(OUTPUT_RATE),
            computer_out: Vec::new(),
        }
    }

    /// Appends the mixed 48 kHz samples to `out`. `computer` is ignored when
    /// the mixer was made without computer audio.
    pub fn process(&mut self, microphone: &[f32], computer: Option<&[f32]>, out: &mut Vec<f32>) {
        let start = out.len();
        self.microphone.process(microphone, out);
        if let (Some(resampler), Some(computer)) = (&mut self.computer, computer) {
            self.computer_out.clear();
            resampler.process(computer, &mut self.computer_out);
            let mixed = &mut out[start..];
            for (sample, other) in mixed.iter_mut().zip(&self.computer_out) {
                *sample += other;
            }
        }
        self.limiter.process(&mut out[start..]);
    }
}

#[cfg(test)]
pub(crate) mod signal {
    use std::f64::consts::PI;

    pub fn sine(freq: f64, amplitude: f32, sample_rate: u32, seconds: f64) -> Vec<f32> {
        let n = (sample_rate as f64 * seconds) as usize;
        (0..n)
            .map(|i| amplitude * (2.0 * PI * freq * i as f64 / sample_rate as f64).sin() as f32)
            .collect()
    }

    pub fn peak(samples: &[f32]) -> f32 {
        samples.iter().fold(0.0, |max, s| max.max(s.abs()))
    }

    /// Amplitude of one frequency, via the Goertzel algorithm. A sine of
    /// amplitude A reads about A.
    pub fn tone_amplitude(samples: &[f32], freq: f64, sample_rate: u32) -> f64 {
        let w = 2.0 * PI * freq / sample_rate as f64;
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in samples {
            let s0 = x as f64 + coeff * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        2.0 * power.max(0.0).sqrt() / samples.len().max(1) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::signal::{peak, sine, tone_amplitude};
    use super::*;

    const RATE: u32 = OUTPUT_RATE;

    fn resample(input: &[f32], from: f64, chunk: usize) -> Vec<f32> {
        let mut resampler = Resampler::new(from, RATE as f64);
        let mut out = Vec::new();
        for part in input.chunks(chunk) {
            resampler.process(part, &mut out);
        }
        out
    }

    #[test]
    fn matching_rates_pass_samples_through_unchanged() {
        let input = sine(440.0, 0.5, RATE, 0.1);
        assert_eq!(resample(&input, 48_000.0, 100), input);
    }

    #[test]
    fn common_microphone_rates_become_48_khz_and_keep_the_tone() {
        for from in [16_000u32, 24_000, 44_100, 96_000] {
            let input = sine(1_000.0, 0.5, from, 1.0);
            let out = resample(&input, from as f64, 512);
            let expected = RATE as usize;
            assert!(
                out.len().abs_diff(expected) <= 2,
                "{from} Hz gave {} samples",
                out.len()
            );
            let amplitude = tone_amplitude(&out, 1_000.0, RATE);
            assert!((amplitude - 0.5).abs() < 0.02, "{from} Hz: {amplitude}");
        }
    }

    #[test]
    fn chunk_size_does_not_change_the_output() {
        let input = sine(1_000.0, 0.5, 44_100, 0.5);
        let whole = resample(&input, 44_100.0, input.len());
        let chunked = resample(&input, 44_100.0, 97);
        assert_eq!(whole.len(), chunked.len());
        for (a, b) in whole.iter().zip(&chunked) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn quiet_signals_pass_the_limiter_untouched() {
        let mut samples = sine(1_000.0, 0.5, RATE, 0.5);
        let before = samples.clone();
        Limiter::new(RATE).process(&mut samples);
        assert_eq!(samples, before);
    }

    #[test]
    fn the_limiter_never_lets_a_sample_past_the_ceiling() {
        let mut samples = sine(1_000.0, 1.8, RATE, 0.5);
        samples.push(f32::NAN);
        samples.push(40.0);
        Limiter::new(RATE).process(&mut samples);
        assert!(samples.iter().all(|s| s.is_finite()));
        assert!(peak(&samples) <= CEILING, "{}", peak(&samples));
        // Limited, not silenced: the tone is still near full scale.
        assert!(tone_amplitude(&samples, 1_000.0, RATE) > 0.8);
    }

    #[test]
    fn the_limiter_recovers_after_a_loud_passage() {
        let mut limiter = Limiter::new(RATE);
        let mut loud = sine(1_000.0, 1.8, RATE, 0.2);
        limiter.process(&mut loud);
        let mut quiet = sine(1_000.0, 0.3, RATE, 2.0);
        limiter.process(&mut quiet);
        let last_half_second = &quiet[quiet.len() - RATE as usize / 2..];
        let amplitude = tone_amplitude(last_half_second, 1_000.0, RATE);
        assert!((amplitude - 0.3).abs() < 0.005, "{amplitude}");
    }

    #[test]
    fn the_mix_contains_both_sources() {
        let microphone = sine(300.0, 0.3, RATE, 1.0);
        let computer = sine(1_000.0, 0.3, RATE, 1.0);
        let mut out = Vec::new();
        Mixer::new(RATE as f64, true).process(&microphone, Some(&computer), &mut out);
        assert_eq!(out.len(), microphone.len());
        for (i, sample) in out.iter().enumerate() {
            assert!((sample - (microphone[i] + computer[i])).abs() < 1e-6);
        }
        assert!((tone_amplitude(&out, 300.0, RATE) - 0.3).abs() < 0.01);
        assert!((tone_amplitude(&out, 1_000.0, RATE) - 0.3).abs() < 0.01);
    }

    #[test]
    fn two_loud_sources_are_limited_instead_of_clipping() {
        let microphone = sine(1_000.0, 0.8, RATE, 1.0);
        let computer = sine(1_000.0, 0.8, RATE, 1.0);
        let mut out = Vec::new();
        Mixer::new(RATE as f64, true).process(&microphone, Some(&computer), &mut out);
        assert!(peak(&out) <= CEILING);
        assert!(peak(&out) > 0.9);
    }

    #[test]
    fn microphone_only_mix_ignores_computer_audio() {
        let microphone = sine(300.0, 0.3, 44_100, 1.0);
        let computer = sine(1_000.0, 0.3, 44_100, 1.0);
        let mut out = Vec::new();
        let mut mixer = Mixer::new(44_100.0, false);
        for (mic, comp) in microphone.chunks(441).zip(computer.chunks(441)) {
            mixer.process(mic, Some(comp), &mut out);
        }
        assert!(out.len().abs_diff(RATE as usize) <= 2);
        assert!((tone_amplitude(&out, 300.0, RATE) - 0.3).abs() < 0.01);
        assert!(tone_amplitude(&out, 1_000.0, RATE) < 0.001);
    }

    #[test]
    fn both_streams_are_resampled_to_48_khz_before_mixing() {
        let microphone = sine(300.0, 0.3, 16_000, 1.0);
        let computer = sine(1_000.0, 0.3, 16_000, 1.0);
        let mut out = Vec::new();
        let mut mixer = Mixer::new(16_000.0, true);
        for (mic, comp) in microphone.chunks(160).zip(computer.chunks(160)) {
            mixer.process(mic, Some(comp), &mut out);
        }
        assert!(out.len().abs_diff(RATE as usize) <= 3);
        assert!((tone_amplitude(&out, 300.0, RATE) - 0.3).abs() < 0.01);
        assert!((tone_amplitude(&out, 1_000.0, RATE) - 0.3).abs() < 0.01);
    }
}
