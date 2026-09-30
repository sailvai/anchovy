//! Writes the mixed track as a 16-bit mono WAV file, the High quality format.
//!
//! The header is rewritten while recording, not only at the end, so the file
//! on disk stays playable up to the last update even if the app is killed.

use std::fs::OpenOptions;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const HEADER_BYTES: u64 = 44;
const BYTES_PER_SAMPLE: u64 = 2;
/// A RIFF file stores its size in 32 bits. At 48 kHz, 16-bit mono this is
/// about 12 hours of audio.
const MAX_DATA_BYTES: u64 = u32::MAX as u64 - (HEADER_BYTES - 8);

pub struct WavWriter<W: Write + Seek> {
    inner: W,
    sample_rate: u32,
    data_bytes: u64,
    /// Reused to convert each chunk without allocating per call.
    encoded: Vec<u8>,
}

/// One float sample as 16-bit PCM. Values past full scale are clamped.
pub fn to_i16(sample: f32) -> i16 {
    if sample.is_nan() {
        return 0;
    }
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

impl<W: Write + Seek> WavWriter<W> {
    /// Writes the header for an empty file.
    pub fn new(inner: W, sample_rate: u32) -> io::Result<Self> {
        let mut writer = WavWriter {
            inner,
            sample_rate,
            data_bytes: 0,
            encoded: Vec::new(),
        };
        let header = writer.header();
        writer.inner.write_all(&header)?;
        Ok(writer)
    }

    fn header(&self) -> [u8; HEADER_BYTES as usize] {
        let data = self.data_bytes as u32;
        let block_align = BYTES_PER_SAMPLE as u16;
        let mut h = [0u8; HEADER_BYTES as usize];
        let fields: [&[u8]; 13] = [
            b"RIFF",
            &(data + (HEADER_BYTES as u32 - 8)).to_le_bytes(),
            b"WAVEfmt ",
            &16u32.to_le_bytes(),
            &1u16.to_le_bytes(), // PCM
            &1u16.to_le_bytes(), // mono
            &self.sample_rate.to_le_bytes(),
            &(self.sample_rate * block_align as u32).to_le_bytes(),
            &block_align.to_le_bytes(),
            &16u16.to_le_bytes(),
            b"data",
            &data.to_le_bytes(),
            &[],
        ];
        let mut at = 0;
        for field in fields {
            h[at..at + field.len()].copy_from_slice(field);
            at += field.len();
        }
        h
    }

    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        let added = samples.len() as u64 * BYTES_PER_SAMPLE;
        if self.data_bytes + added > MAX_DATA_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "the recording reached the WAV size limit of about 12 hours",
            ));
        }
        self.encoded.clear();
        for &sample in samples {
            self.encoded
                .extend_from_slice(&to_i16(sample).to_le_bytes());
        }
        self.inner.write_all(&self.encoded)?;
        self.data_bytes += added;
        Ok(())
    }

    /// Size of the file so far, header included.
    pub fn bytes(&self) -> u64 {
        HEADER_BYTES + self.data_bytes
    }

    /// Seconds of audio written so far.
    pub fn seconds(&self) -> f64 {
        (self.data_bytes / BYTES_PER_SAMPLE) as f64 / self.sample_rate as f64
    }

    /// Writes the current sizes into the header and flushes, so the file on
    /// disk is complete up to here.
    pub fn update_header(&mut self) -> io::Result<()> {
        let header = self.header();
        self.inner.seek(SeekFrom::Start(0))?;
        self.inner.write_all(&header)?;
        self.inner.seek(SeekFrom::Start(self.bytes()))?;
        self.inner.flush()
    }

    /// Finishes the file and returns the underlying writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.update_header()?;
        Ok(self.inner)
    }
}

/// Makes the header of a WAV this writer wrote agree with the samples on
/// disk, as after a crash, when the last header update may be a second
/// behind. Returns the seconds of audio in it.
pub fn repair_header(path: &Path) -> io::Result<f64> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut header = [0u8; HEADER_BYTES as usize];
    file.read_exact(&mut header)?;
    if &header[..4] != b"RIFF" || &header[8..16] != b"WAVEfmt " || &header[36..40] != b"data" {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a WAV file Anchovy wrote",
        ));
    }
    let rate = u32::from_le_bytes(header[24..28].try_into().unwrap());
    let len = file.metadata()?.len();
    // Whole samples only, and never past what a RIFF size can say.
    let data = (len.saturating_sub(HEADER_BYTES) / BYTES_PER_SAMPLE * BYTES_PER_SAMPLE)
        .min(MAX_DATA_BYTES / BYTES_PER_SAMPLE * BYTES_PER_SAMPLE);
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(data as u32 + (HEADER_BYTES as u32 - 8)).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&(data as u32).to_le_bytes())?;
    file.sync_all()?;
    if rate == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "no sample rate"));
    }
    Ok((data / BYTES_PER_SAMPLE) as f64 / f64::from(rate))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn u16_at(bytes: &[u8], at: usize) -> u16 {
        u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
    }

    fn u32_at(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    fn samples(bytes: &[u8]) -> Vec<i16> {
        bytes[HEADER_BYTES as usize..]
            .chunks(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[test]
    fn header_describes_48_khz_16_bit_mono_pcm() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        writer.write(&[0.0; 10]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(u32_at(&bytes, 4), 36 + 20);
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32_at(&bytes, 16), 16);
        assert_eq!(u16_at(&bytes, 20), 1, "PCM");
        assert_eq!(u16_at(&bytes, 22), 1, "mono");
        assert_eq!(u32_at(&bytes, 24), 48_000);
        assert_eq!(u32_at(&bytes, 28), 96_000, "byte rate");
        assert_eq!(u16_at(&bytes, 32), 2, "block align");
        assert_eq!(u16_at(&bytes, 34), 16, "bits per sample");
        assert_eq!(&bytes[36..40], b"data");
        assert_eq!(u32_at(&bytes, 40), 20);
        assert_eq!(bytes.len(), 44 + 20);
    }

    #[test]
    fn samples_are_scaled_rounded_and_clamped() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        writer
            .write(&[0.0, 1.0, -1.0, 0.5, -0.25, 3.0, -3.0, f32::NAN])
            .unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(
            samples(&bytes),
            vec![0, 32767, -32767, 16384, -8192, 32767, -32767, 0]
        );
    }

    #[test]
    fn chunks_are_written_in_order() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        writer.write(&[0.5]).unwrap();
        writer.write(&[]).unwrap();
        writer.write(&[-0.5, 1.0]).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(samples(&bytes), vec![16384, -16384, 32767]);
    }

    #[test]
    fn size_and_length_count_what_was_written() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        assert_eq!(writer.bytes(), 44);
        writer.write(&vec![0.1; 72_000]).unwrap();
        assert_eq!(writer.bytes(), 44 + 144_000);
        assert_eq!(writer.seconds(), 1.5);
    }

    #[test]
    fn the_header_is_correct_after_each_update_while_recording() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        writer.write(&[0.25; 6]).unwrap();
        writer.update_header().unwrap();
        writer.write(&[0.25; 4]).unwrap();
        // The header was updated after 6 samples; the 4 after it are appended,
        // not written over the header.
        let snapshot = writer.inner.get_ref().clone();
        assert_eq!(u32_at(&snapshot, 40), 12);
        assert_eq!(u32_at(&snapshot, 4), 36 + 12);
        assert_eq!(snapshot.len(), 44 + 20);

        let bytes = writer.finish().unwrap().into_inner();
        assert_eq!(u32_at(&bytes, 40), 20);
        assert_eq!(samples(&bytes), vec![8192; 10]);
    }

    #[test]
    fn writing_past_the_wav_size_limit_is_an_error() {
        let mut writer = WavWriter::new(Cursor::new(Vec::new()), 48_000).unwrap();
        writer.data_bytes = MAX_DATA_BYTES - 2;
        writer.write(&[0.0]).unwrap();
        let err = writer.write(&[0.0]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn a_header_left_behind_by_a_killed_app_is_repaired() {
        let dir = crate::notes::test_dir::TestDir::new();
        let path = dir.path().join("recording.wav");
        let mut writer = WavWriter::new(std::fs::File::create(&path).unwrap(), 48_000).unwrap();
        writer.write(&[0.5; 24_000]).unwrap();
        writer.update_header().unwrap();
        // Written after the last update, then the app was killed.
        writer.write(&[0.5; 24_000]).unwrap();
        drop(writer);
        let mut bytes = std::fs::read(&path).unwrap();
        // A half sample at the end, cut off mid-write.
        bytes.push(0x7f);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(u32_at(&bytes, 40), 48_000);

        let seconds = repair_header(&path).unwrap();

        assert_eq!(seconds, 1.0);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(u32_at(&bytes, 40), 96_000);
        assert_eq!(u32_at(&bytes, 4), 36 + 96_000);
        assert_eq!(samples(&bytes[..44 + 96_000]).len(), 48_000);
    }

    #[test]
    fn repair_refuses_a_file_it_did_not_write() {
        let dir = crate::notes::test_dir::TestDir::new();
        let path = dir.path().join("notes.md");
        std::fs::write(&path, vec![b'#'; 100]).unwrap();
        assert_eq!(
            repair_header(&path).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(std::fs::read(&path).unwrap(), vec![b'#'; 100]);
    }
}
