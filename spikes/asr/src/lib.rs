//! Plan step 6a spike: the testable half. Reads a 16 kHz mono WAV clip,
//! checks model files against their sha256, splits Qwen3-ASR's raw output into
//! language and text, scores a transcript against the script it was read from,
//! and turns one run's timings into an estimate for one hour of audio.
//! The llama.cpp calls live in `main.rs`.
//!
//! Not the engine: no chunking of long audio, no timestamps, no overlap.

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

pub const SAMPLE_RATE: u32 = 16_000;

/// Samples of a 16-bit PCM, 16 kHz, mono WAV file, scaled to -1.0..1.0.
pub fn read_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut format = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                let tag = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([body[14], body[15]]);
                format = Some((tag, channels, rate, bits));
            }
            b"data" => {
                if format != Some((1, 1, SAMPLE_RATE, 16)) {
                    return Err(format!(
                        "expected 16-bit PCM, mono, {SAMPLE_RATE} Hz; got {format:?} \
                         as (format, channels, rate, bits)"
                    ));
                }
                return Ok(body
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0)
                    .collect());
            }
            _ => {}
        }
        // Chunks are padded to an even length.
        pos += 8 + len + (len & 1);
    }
    Err("no data chunk".into())
}

pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buf[..n]);
    }
}

/// Qwen3-ASR answers `language <Name><asr_text><transcript>`.
#[derive(Debug, PartialEq, Eq)]
pub struct AsrOutput {
    pub language: Option<String>,
    pub text: String,
}

pub fn parse_output(raw: &str) -> AsrOutput {
    match raw.split_once("<asr_text>") {
        Some((head, text)) => AsrOutput {
            language: head
                .trim()
                .strip_prefix("language")
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
            text: text.trim().to_string(),
        },
        None => AsrOutput {
            language: None,
            text: raw.trim().to_string(),
        },
    }
}

/// Characters for Chinese, words for English. Punctuation and case are ignored.
pub fn units(text: &str, by_word: bool) -> Vec<String> {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '\'' {
                c.to_lowercase().next().unwrap()
            } else {
                ' '
            }
        })
        .collect();
    if by_word {
        clean.split_whitespace().map(str::to_string).collect()
    } else {
        clean
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(String::from)
            .collect()
    }
}

/// Edit distance divided by the reference length: CER on characters, WER on words.
pub fn error_rate(reference: &[String], hypothesis: &[String]) -> f64 {
    if reference.is_empty() {
        return if hypothesis.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=hypothesis.len()).collect();
    for (i, r) in reference.iter().enumerate() {
        let mut row = vec![i + 1; hypothesis.len() + 1];
        for (j, h) in hypothesis.iter().enumerate() {
            let substitute = prev[j] + usize::from(r != h);
            row[j + 1] = substitute.min(prev[j + 1] + 1).min(row[j] + 1);
        }
        prev = row;
    }
    prev[hypothesis.len()] as f64 / reference.len() as f64
}

/// Seconds to transcribe one hour, if work scales with audio length:
/// one model load plus the measured time per second of audio.
pub fn hour_estimate(load_secs: f64, work_secs: f64, audio_secs: f64) -> f64 {
    load_secs + work_secs / audio_secs * 3600.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(tag: u16, channels: u16, rate: u32, bits: u16, samples: &[i16]) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
        // An odd-length chunk before fmt, to check padding.
        out.extend(b"LIST\x03\0\0\0abc\0");
        out.extend(b"fmt \x10\0\0\0");
        out.extend(tag.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * u32::from(bits / 8) * u32::from(channels)).to_le_bytes());
        out.extend((bits / 8 * channels).to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend((samples.len() as u32 * 2).to_le_bytes());
        for s in samples {
            out.extend(s.to_le_bytes());
        }
        out
    }

    #[test]
    fn reads_16k_mono_pcm() {
        let samples = read_wav(&wav(1, 1, 16_000, 16, &[0, 16384, -32768])).unwrap();
        assert_eq!(samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn rejects_other_formats() {
        assert!(read_wav(&wav(1, 2, 16_000, 16, &[0, 0])).is_err());
        assert!(read_wav(&wav(1, 1, 44_100, 16, &[0])).is_err());
        assert!(read_wav(b"not a wav").is_err());
    }

    #[test]
    fn hashes_a_file() {
        let path = std::env::temp_dir().join(format!("asr-spike-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        let hash = sha256_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            hash,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn splits_language_and_text() {
        assert_eq!(
            parse_output("language Chinese<asr_text>你好。"),
            AsrOutput {
                language: Some("Chinese".into()),
                text: "你好。".into()
            }
        );
        assert_eq!(
            parse_output(" hello "),
            AsrOutput {
                language: None,
                text: "hello".into()
            }
        );
    }

    #[test]
    fn scores_characters_and_words() {
        let r = units("我们下周一开会。", false);
        assert_eq!(r.len(), 7);
        assert_eq!(error_rate(&r, &units("我们下周二开会", false)), 1.0 / 7.0);
        let r = units("Ship it on Friday.", true);
        assert_eq!(error_rate(&r, &units("ship it friday", true)), 0.25);
        assert_eq!(error_rate(&r, &r), 0.0);
    }

    #[test]
    fn estimates_one_hour() {
        // 30 s of audio in 3 s of work, 2 s to load: 362 s.
        assert_eq!(hour_estimate(2.0, 3.0, 30.0), 362.0);
    }
}
