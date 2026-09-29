//! Reads a recording and converts it to what the transcription models take:
//! 16 kHz mono floats. The file on disk is never changed.
//!
//! The recording is read in blocks and resampled as it is read, so an hour
//! of 48 kHz audio never sits in memory at full rate.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// The sample rate the transcription models take.
pub const MODEL_RATE: u32 = 16_000;

#[derive(Debug)]
pub enum AudioError {
    Io(io::Error),
    /// Not a WAV file this reader understands.
    Unsupported(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AudioError::Io(err) => write!(f, "Anchovy can't read the audio. {err}"),
            AudioError::Unsupported(what) => write!(f, "Anchovy can't read this audio: {what}."),
        }
    }
}

impl std::error::Error for AudioError {}

impl From<io::Error> for AudioError {
    fn from(err: io::Error) -> Self {
        AudioError::Io(err)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Pcm16,
    Float32,
}

impl Encoding {
    fn bytes(self) -> usize {
        match self {
            Encoding::Pcm16 => 2,
            Encoding::Float32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Format {
    rate: u32,
    channels: u16,
    encoding: Encoding,
    /// Byte offset of the first sample.
    data_start: u64,
    /// Bytes of samples. While recording, or if the app was killed, the
    /// header may say 0 or too much; the bytes on disk are the truth then.
    data_len: u64,
}

fn read_array<const N: usize>(reader: &mut impl Read) -> io::Result<[u8; N]> {
    let mut buf = [0; N];
    reader.read_exact(&mut buf)?;
    Ok(buf)
}

fn read_format(reader: &mut (impl Read + Seek), file_len: u64) -> Result<Format, AudioError> {
    let unsupported = |what: &str| AudioError::Unsupported(what.into());
    let header: [u8; 12] = read_array(reader).map_err(|_| unsupported("not a WAV file"))?;
    if &header[..4] != b"RIFF" || &header[8..] != b"WAVE" {
        return Err(unsupported("not a WAV file"));
    }
    let mut fmt = None;
    loop {
        let chunk: [u8; 8] = read_array(reader).map_err(|_| unsupported("no audio data"))?;
        let size = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as u64;
        match &chunk[..4] {
            b"fmt " => {
                if size < 16 {
                    return Err(unsupported("a broken format header"));
                }
                let body: [u8; 16] = read_array(reader)?;
                let tag = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([body[14], body[15]]);
                // 0xFFFE is WAVE_FORMAT_EXTENSIBLE; its sub-format is not
                // checked, and the bit depth decides.
                let encoding = match (tag, bits) {
                    (1 | 0xFFFE, 16) => Encoding::Pcm16,
                    (3 | 0xFFFE, 32) => Encoding::Float32,
                    _ => return Err(unsupported(&format!("{bits}-bit audio, format {tag}"))),
                };
                if channels == 0 || rate == 0 {
                    return Err(unsupported("a broken format header"));
                }
                fmt = Some((rate, channels, encoding));
                reader.seek(SeekFrom::Current((size - 16 + (size & 1)) as i64))?;
            }
            b"data" => {
                let (rate, channels, encoding) =
                    fmt.ok_or_else(|| unsupported("no format header"))?;
                let data_start = reader.stream_position()?;
                let on_disk = file_len.saturating_sub(data_start);
                let data_len = match size {
                    0 | 0xFFFF_FFFF => on_disk,
                    size => size.min(on_disk),
                };
                return Ok(Format {
                    rate,
                    channels,
                    encoding,
                    data_start,
                    data_len,
                });
            }
            _ => {
                reader.seek(SeekFrom::Current((size + (size & 1)) as i64))?;
            }
        }
    }
}

/// Reads a WAV file as 16 kHz mono: channels are averaged, then resampled.
pub fn read_model_audio(path: &Path) -> Result<Vec<f32>, AudioError> {
    let file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(1 << 16, file);
    let format = read_format(&mut reader, file_len)?;
    reader.seek(SeekFrom::Start(format.data_start))?;

    let frame_bytes = format.encoding.bytes() * format.channels as usize;
    let frames = format.data_len / frame_bytes as u64;
    let mut resampler = Resampler::new(format.rate, MODEL_RATE);
    let mut out = Vec::with_capacity(resampler.output_len(frames) as usize);
    let mut bytes = vec![0u8; frame_bytes * 16_384];
    let mut mono = Vec::with_capacity(16_384);
    let mut left = frames;
    while left > 0 {
        let take = left.min(16_384) as usize;
        let block = &mut bytes[..take * frame_bytes];
        reader.read_exact(block)?;
        mono.clear();
        mono.extend(block.chunks_exact(frame_bytes).map(|frame| {
            let sum: f32 = frame
                .chunks_exact(format.encoding.bytes())
                .map(|sample| match format.encoding {
                    Encoding::Pcm16 => {
                        f32::from(i16::from_le_bytes([sample[0], sample[1]])) / 32768.0
                    }
                    Encoding::Float32 => f32::from_le_bytes(sample.try_into().unwrap()),
                })
                .sum();
            sum / f32::from(format.channels)
        }));
        resampler.push(&mono, &mut out);
        left -= take as u64;
    }
    resampler.finish(&mut out);
    Ok(out)
}

/// Zero crossings of the windowed-sinc kernel on each side. More is sharper
/// and slower; 16 keeps speech bands flat and the alias well below them.
const ZERO_CROSSINGS: f64 = 16.0;
/// The low-pass sits a little below the output's Nyquist frequency.
const ROLLOFF: f64 = 0.95;

/// A streaming windowed-sinc resampler from `from` Hz to `to` Hz. Input may
/// arrive in blocks of any size; the output is the same either way.
struct Resampler {
    /// Input samples per output sample.
    step: f64,
    /// Low-pass cutoff as a fraction of the input Nyquist frequency.
    cutoff: f64,
    /// Kernel half-width in input samples.
    half_width: f64,
    /// Input not yet fully used. `buf[0]` is input sample `buf_start`.
    buf: Vec<f32>,
    buf_start: u64,
    /// Input samples seen so far.
    seen: u64,
    /// Output samples produced so far.
    produced: u64,
    passthrough: bool,
}

impl Resampler {
    fn new(from: u32, to: u32) -> Self {
        let step = f64::from(from) / f64::from(to);
        let cutoff = (1.0 / step).min(1.0) * ROLLOFF;
        Resampler {
            step,
            cutoff,
            half_width: ZERO_CROSSINGS / cutoff,
            buf: Vec::new(),
            buf_start: 0,
            seen: 0,
            produced: 0,
            passthrough: from == to,
        }
    }

    /// Output samples for `frames` input samples.
    fn output_len(&self, frames: u64) -> u64 {
        (frames as f64 / self.step).ceil() as u64
    }

    fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        self.seen += input.len() as u64;
        if self.passthrough {
            out.extend_from_slice(input);
            self.produced = self.seen;
            return;
        }
        self.buf.extend_from_slice(input);
        self.produce(out, false);
    }

    fn finish(&mut self, out: &mut Vec<f32>) {
        if !self.passthrough {
            self.produce(out, true);
        }
    }

    fn produce(&mut self, out: &mut Vec<f32>, at_end: bool) {
        let total = self.output_len(self.seen);
        while self.produced < total {
            let center = self.produced as f64 * self.step;
            let last = (center + self.half_width).floor() as u64;
            if !at_end && last >= self.seen {
                break;
            }
            out.push(self.sample_at(center));
            self.produced += 1;
        }
        // Keep only the input the next output still needs.
        let next_center = self.produced as f64 * self.step;
        let first_needed = (next_center - self.half_width).ceil().max(0.0) as u64;
        if first_needed > self.buf_start {
            let drop = ((first_needed - self.buf_start) as usize).min(self.buf.len());
            self.buf.drain(..drop);
            self.buf_start += drop as u64;
        }
    }

    fn sample_at(&self, center: f64) -> f32 {
        let first = (center - self.half_width).ceil().max(0.0) as u64;
        let last = (center + self.half_width).floor() as u64;
        let mut sum = 0.0f64;
        let mut i = first.max(self.buf_start);
        while i <= last {
            let Some(&x) = self.buf.get((i - self.buf_start) as usize) else {
                break;
            };
            sum += f64::from(x) * self.kernel(i as f64 - center);
            i += 1;
        }
        sum as f32
    }

    /// Low-pass sinc at `cutoff`, shaped by a Blackman window.
    fn kernel(&self, x: f64) -> f64 {
        use std::f64::consts::PI;
        let t = x / self.half_width;
        if t.abs() >= 1.0 {
            return 0.0;
        }
        let window = 0.42 + 0.5 * (PI * t).cos() + 0.08 * (2.0 * PI * t).cos();
        let arg = PI * self.cutoff * x;
        let sinc = if arg.abs() < 1e-12 {
            1.0
        } else {
            arg.sin() / arg
        };
        self.cutoff * sinc * window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::test_dir::TestDir;
    use std::f32::consts::PI;

    fn wav(rate: u32, channels: u16, tag: u16, bits: u16, data: &[u8], data_size: u32) -> Vec<u8> {
        let mut out = b"RIFF\0\0\0\0WAVE".to_vec();
        // An odd-sized chunk before fmt, to check padding.
        out.extend(b"LIST\x03\0\0\0abc\0");
        out.extend(b"fmt \x10\0\0\0");
        out.extend(tag.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        let block = u32::from(bits / 8) * u32::from(channels);
        out.extend((rate * block).to_le_bytes());
        out.extend((block as u16).to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend(data_size.to_le_bytes());
        out.extend(data);
        out
    }

    fn pcm16(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|s| ((s * 32767.0).round() as i16).to_le_bytes())
            .collect()
    }

    fn write(dir: &TestDir, bytes: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join("audio.wav");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn sine(freq: f32, rate: u32, seconds: f32, amplitude: f32) -> Vec<f32> {
        (0..(rate as f32 * seconds) as usize)
            .map(|i| amplitude * (2.0 * PI * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn rising_zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|w| w[0] < 0.0 && w[1] >= 0.0)
            .count()
    }

    #[test]
    fn sixteen_khz_mono_passes_through() {
        let dir = TestDir::new();
        let samples = [0.0, 0.5, -0.5, 0.25];
        let data = pcm16(&samples);
        let path = write(&dir, &wav(16_000, 1, 1, 16, &data, data.len() as u32));

        let out = read_model_audio(&path).unwrap();

        assert_eq!(out.len(), 4);
        for (a, b) in out.iter().zip(samples) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
    }

    #[test]
    fn channels_are_averaged() {
        let dir = TestDir::new();
        // Left 0.5, right -0.25, twice.
        let data = pcm16(&[0.5, -0.25, 0.5, -0.25]);
        let path = write(&dir, &wav(16_000, 2, 1, 16, &data, data.len() as u32));

        let out = read_model_audio(&path).unwrap();

        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|s| (s - 0.125).abs() < 1e-4), "{out:?}");
    }

    #[test]
    fn a_48_khz_recording_becomes_16_khz_with_speech_kept() {
        let dir = TestDir::new();
        let input = sine(1000.0, 48_000, 1.0, 0.5);
        let data = pcm16(&input);
        let path = write(&dir, &wav(48_000, 1, 1, 16, &data, data.len() as u32));

        let out = read_model_audio(&path).unwrap();

        assert_eq!(out.len(), 16_000);
        // Away from the edges, a 1 kHz tone keeps its level and pitch.
        let middle = &out[1600..14_400];
        assert!(
            (rms(middle) - 0.5 / 2f32.sqrt()).abs() < 0.01,
            "rms {}",
            rms(middle)
        );
        let crossings = rising_zero_crossings(middle);
        assert!((799..=801).contains(&crossings), "{crossings} crossings");
    }

    #[test]
    fn tones_above_8_khz_do_not_fold_back_into_speech() {
        let dir = TestDir::new();
        let input = sine(12_000.0, 48_000, 1.0, 0.5);
        let data = pcm16(&input);
        let path = write(&dir, &wav(48_000, 1, 1, 16, &data, data.len() as u32));

        let out = read_model_audio(&path).unwrap();

        assert!(
            rms(&out[1600..14_400]) < 0.005,
            "rms {}",
            rms(&out[1600..14_400])
        );
    }

    #[test]
    fn block_size_does_not_change_the_output() {
        let input = sine(440.0, 44_100, 0.5, 0.3);
        let mut whole = Vec::new();
        let mut resampler = Resampler::new(44_100, 16_000);
        resampler.push(&input, &mut whole);
        resampler.finish(&mut whole);

        let mut pieces = Vec::new();
        let mut resampler = Resampler::new(44_100, 16_000);
        for block in input.chunks(97) {
            resampler.push(block, &mut pieces);
        }
        resampler.finish(&mut pieces);

        assert_eq!(
            whole.len(),
            (input.len() as f64 * 16_000.0 / 44_100.0).ceil() as usize
        );
        assert_eq!(whole, pieces);
    }

    #[test]
    fn float_wav_files_are_read() {
        let dir = TestDir::new();
        let data: Vec<u8> = [0.25f32, -0.75]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let path = write(&dir, &wav(16_000, 1, 3, 32, &data, data.len() as u32));

        assert_eq!(read_model_audio(&path).unwrap(), vec![0.25, -0.75]);
    }

    #[test]
    fn a_recording_cut_off_before_its_header_was_final_is_read_to_the_end() {
        let dir = TestDir::new();
        let data = pcm16(&[0.5; 10]);
        for size in [0, 0xFFFF_FFFF, 1_000_000] {
            let path = write(&dir, &wav(16_000, 1, 1, 16, &data, size));
            assert_eq!(read_model_audio(&path).unwrap().len(), 10, "size {size}");
        }
    }

    #[test]
    fn other_files_are_refused_with_a_reason() {
        let dir = TestDir::new();
        let path = write(&dir, b"....ftypM4A ");
        let err = read_model_audio(&path).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Anchovy can't read this audio: not a WAV file."
        );

        let path = write(&dir, &wav(16_000, 1, 1, 24, &[0; 6], 6));
        let err = read_model_audio(&path).unwrap_err();
        assert_eq!(
            err.to_string(),
            "Anchovy can't read this audio: 24-bit audio, format 1."
        );
    }
}
