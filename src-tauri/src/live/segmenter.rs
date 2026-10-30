//! Where a phrase starts and ends, from 16 kHz mono samples.
//!
//! Each 20 ms frame is sound or silence by its root mean square against a
//! noise floor: the quietest frame of the last few seconds. A frame well
//! above the floor is sound. After sound, enough silence in a row ends the
//! phrase. Very short sounds and very quiet phrases never reach the model.
//! Samples can arrive in pieces of any size; the phrases are the same.

use std::collections::VecDeque;

use serde::Serialize;

use super::Phrase;
use crate::engines::audio::MODEL_RATE;

/// A frame or phrase this quiet holds no speech. The same value as
/// `SILENCE_RMS` in `pipeline.rs`, which skips silent windows of a note.
pub const SILENCE_RMS: f32 = 1e-4;

/// The numbers that decide where phrases end.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Params {
    /// Length of one frame.
    pub frame_seconds: f64,
    /// The noise floor is the quietest frame of this many seconds.
    pub floor_seconds: f64,
    /// A frame this many decibels above the floor is sound.
    pub sound_db: f64,
    /// This much silence in a row after sound ends a phrase.
    pub end_silence_seconds: f64,
    /// From the first to the last frame of sound, a phrase shorter than
    /// this is dropped.
    pub min_sound_seconds: f64,
    /// A phrase that gets this long ends here, matching `WINDOW_SECONDS` in
    /// `engines/windows.rs`.
    pub max_phrase_seconds: f64,
    /// Silence kept before and after the sound, so soft starts and endings
    /// are not cut.
    pub pad_seconds: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            frame_seconds: 0.02,
            floor_seconds: 3.0,
            sound_db: 12.0,
            end_silence_seconds: 0.6,
            min_sound_seconds: 0.4,
            max_phrase_seconds: 30.0,
            pad_seconds: 0.2,
        }
    }
}

pub struct Segmenter {
    _params: Params,
}

impl Segmenter {
    pub fn new(params: Params) -> Self {
        Segmenter { _params: params }
    }

    /// Feeds the next samples and returns every phrase they finished.
    pub fn push(&mut self, _samples: &[f32]) -> Vec<Phrase> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = MODEL_RATE as f64;
    const FRAME: usize = 320;

    fn at(seconds: f64) -> usize {
        (seconds * RATE).round() as usize
    }

    fn silence(seconds: f64) -> Vec<f32> {
        vec![0.0; at(seconds)]
    }

    fn tone(seconds: f64) -> Vec<f32> {
        (0..at(seconds))
            .map(|i| 0.3 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin())
            .collect()
    }

    /// Like speech: 0.25 s of tone, then 0.05 s of silence, over and over.
    /// There is never a pause long enough to end a phrase.
    fn syllables(seconds: f64) -> Vec<f32> {
        let mut out = Vec::new();
        while out.len() < at(seconds) {
            out.extend(tone(0.25));
            out.extend(silence(0.05));
        }
        out.truncate(at(seconds));
        out
    }

    /// White noise with this root mean square, the same on every run.
    fn noise(seconds: f64, rms: f32, seed: &mut u64) -> Vec<f32> {
        (0..at(seconds))
            .map(|_| {
                *seed ^= *seed << 13;
                *seed ^= *seed >> 7;
                *seed ^= *seed << 17;
                let uniform = (*seed >> 11) as f32 / (1u64 << 53) as f32 * 2.0 - 1.0;
                uniform * rms * 3f32.sqrt()
            })
            .collect()
    }

    fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
        a.iter().zip(b).map(|(x, y)| x + y).collect()
    }

    fn join(parts: &[Vec<f32>]) -> Vec<f32> {
        parts.concat()
    }

    fn phrases(samples: &[f32]) -> Vec<Phrase> {
        Segmenter::new(Params::default()).push(samples)
    }

    #[test]
    fn silence_of_0_6_seconds_or_more_splits_two_sounds() {
        for gap in [0.6, 1.0] {
            let audio = join(&[
                silence(1.0),
                tone(1.0),
                silence(gap),
                tone(1.0),
                silence(1.0),
            ]);
            let found = phrases(&audio);
            assert_eq!(found.len(), 2, "gap {gap}");
            assert_eq!(found[0].voice_end, at(2.0));
            assert_eq!(found[0].cut, at(2.6), "ends 0.6 s after the sound");
            assert_eq!(found[1].voice_end, at(3.0 + gap));
        }
    }

    #[test]
    fn a_phrase_keeps_0_2_seconds_of_silence_on_either_side() {
        let audio = join(&[silence(1.0), tone(1.0), silence(1.0)]);
        let found = phrases(&audio);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, at(0.8));
        assert_eq!(found[0].samples, audio[at(0.8)..at(2.2)]);
    }

    #[test]
    fn a_pause_of_0_3_seconds_does_not_split() {
        let audio = join(&[
            silence(1.0),
            tone(1.0),
            silence(0.3),
            tone(1.0),
            silence(1.0),
        ]);
        let found = phrases(&audio);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].start, at(0.8));
        assert_eq!(found[0].voice_end, at(3.3));
    }

    #[test]
    fn a_click_of_0_2_seconds_is_not_a_phrase() {
        let audio = join(&[silence(0.5), tone(0.2), silence(1.0)]);
        assert_eq!(phrases(&audio), vec![]);
        let audio = join(&[silence(0.5), tone(0.4), silence(1.0)]);
        assert_eq!(phrases(&audio).len(), 1, "0.4 s is long enough");
    }

    #[test]
    fn sound_quieter_than_the_pipelines_silence_is_never_a_phrase() {
        let quiet: Vec<f32> = tone(2.0).iter().map(|s| s * 2e-4).collect();
        let audio = join(&[silence(1.0), quiet, silence(1.0)]);
        assert_eq!(phrases(&audio), vec![]);
    }

    #[test]
    fn silence_rms_matches_the_pipeline() {
        let source = include_str!("../pipeline.rs");
        let line = source
            .lines()
            .find(|line| line.starts_with("const SILENCE_RMS: f32 = "))
            .expect("pipeline.rs defines SILENCE_RMS");
        let value: f32 = line["const SILENCE_RMS: f32 = ".len()..]
            .trim_end_matches(';')
            .parse()
            .unwrap();
        assert_eq!(value, SILENCE_RMS);
    }

    #[test]
    fn continuous_sound_ends_at_30_seconds() {
        let audio = join(&[syllables(31.0), silence(1.0)]);
        let found = phrases(&audio);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].start, 0);
        assert_eq!(found[0].cut, at(30.0));
        assert_eq!(found[0].samples.len(), at(30.0));
        assert_eq!(found[1].start, at(30.0), "the rest is not heard twice");
        assert_eq!(found[1].voice_end, at(31.0));
    }

    #[test]
    fn still_splits_after_the_noise_floor_rises_20_db() {
        let mut seed = 0x5eed;
        let quiet = noise(3.0, 0.001, &mut seed);
        let loud = noise(5.0, 0.01, &mut seed);
        let speech = join(&[tone(1.0), silence(0.8), tone(1.0), silence(1.5)]);
        let under = noise(4.3, 0.01, &mut seed);
        let audio = join(&[quiet, loud, mix(&speech, &under)]);
        let found = phrases(&audio);
        // While the floor catches up, the louder noise may count as one
        // phrase. After that, the two tones are two phrases.
        assert!(found.len() <= 3, "{} phrases", found.len());
        let tones: Vec<&Phrase> = found.iter().filter(|p| p.voice_end > at(8.0)).collect();
        assert_eq!(tones.len(), 2);
        assert_eq!(tones[0].voice_end, at(9.0));
        assert_eq!(tones[1].voice_end, at(10.8));
        assert!(found
            .iter()
            .all(|p| p.voice_end <= at(8.0) || p.start >= at(7.8)));
    }

    #[test]
    fn pieces_of_any_size_give_the_same_phrases_as_all_at_once() {
        let mut seed = 7;
        let base = join(&[
            silence(0.7),
            tone(1.3),
            silence(0.3),
            tone(0.9),
            silence(0.8),
            tone(0.2),
            silence(1.1),
            syllables(31.5),
            silence(0.9),
        ]);
        let len = base.len();
        let audio = mix(&base, &noise(len as f64 / RATE, 0.002, &mut seed));
        let whole = phrases(&audio);
        assert_eq!(whole.len(), 3);

        let mut segmenter = Segmenter::new(Params::default());
        let mut pieces = Vec::new();
        let sizes = [1, 7, FRAME, 333, 4410, FRAME - 1, 16_000];
        let mut at = 0;
        for size in sizes.iter().cycle() {
            if at >= audio.len() {
                break;
            }
            let end = (at + size).min(audio.len());
            pieces.extend(segmenter.push(&audio[at..end]));
            at = end;
        }
        assert_eq!(pieces, whole);
    }

    #[test]
    fn silence_alone_gives_nothing() {
        assert_eq!(phrases(&silence(10.0)), vec![]);
    }
}
