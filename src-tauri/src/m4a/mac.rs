//! The macOS AAC encoder and decoder, through ExtAudioFile in AudioToolbox.
//! Kept thin: when to encode and what to do when it fails is in
//! `recording::small`, and turning decoded audio into what the models take
//! is in `engines::audio`.

use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;
use std::ptr::{self, NonNull};

use objc2_audio_toolbox::{
    kAudioFileM4AType, kExtAudioFileProperty_ClientDataFormat,
    kExtAudioFileProperty_FileDataFormat, kExtAudioFileProperty_FileLengthFrames, AudioFileFlags,
    ExtAudioFileCreateWithURL, ExtAudioFileDispose, ExtAudioFileGetProperty, ExtAudioFileOpenURL,
    ExtAudioFilePropertyID, ExtAudioFileRead, ExtAudioFileRef, ExtAudioFileSetProperty,
    ExtAudioFileWrite,
};
use objc2_core_audio_types::{
    kAudioFormatFlagIsFloat, kAudioFormatFlagIsPacked, kAudioFormatLinearPCM, kAudioFormatMPEG4AAC,
    AudioBuffer, AudioBufferList, AudioStreamBasicDescription,
};
use objc2_core_foundation::CFURL;

use crate::recording::mixer::OUTPUT_RATE;
use crate::recording::small::Encoder;

/// Frames moved per read and write.
const BLOCK_FRAMES: usize = 8192;

/// The system encoder.
pub struct MacEncoder;

impl Encoder for MacEncoder {
    fn encode(&self, wav: &Path, m4a: &Path) -> Result<(), String> {
        let input = AudioFile::open(wav)?;
        let client = float_format(f64::from(OUTPUT_RATE), 1);
        input.set_client_format(&client)?;
        let output = AudioFile::create_m4a(m4a, &aac_mono(f64::from(OUTPUT_RATE)))?;
        output.set_client_format(&client)?;
        let mut samples = vec![0f32; BLOCK_FRAMES];
        loop {
            let frames = input.read(&mut samples, 1)?;
            if frames == 0 {
                break;
            }
            output.write(&mut samples[..frames], 1)?;
        }
        drop(input);
        // Closing writes the M4A's index; until then it does not play.
        output.close()
    }

    fn duration(&self, audio: &Path) -> Result<f64, String> {
        let file = AudioFile::open(audio)?;
        let format = file.file_format()?;
        let frames: i64 = file.get(kExtAudioFileProperty_FileLengthFrames)?;
        if format.mSampleRate <= 0.0 {
            return Err("The audio file has no sample rate.".into());
        }
        Ok(frames as f64 / format.mSampleRate)
    }
}

/// Reads an audio file in blocks as interleaved floats at the file's own
/// sample rate and channel count, so the caller mixes and resamples it the
/// same way as a WAV.
pub struct Decoder {
    file: AudioFile,
    sample_rate: f64,
    channels: u32,
}

impl Decoder {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = AudioFile::open(path)?;
        let format = file.file_format()?;
        let channels = format.mChannelsPerFrame;
        if format.mSampleRate <= 0.0 || channels == 0 {
            return Err("the file has no audio".into());
        }
        file.set_client_format(&float_format(format.mSampleRate, channels))?;
        Ok(Decoder {
            file,
            sample_rate: format.mSampleRate,
            channels,
        })
    }

    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Fills `samples` with whole frames; returns how many. 0 at the end.
    pub fn read(&mut self, samples: &mut [f32]) -> Result<usize, String> {
        self.file.read(samples, self.channels)
    }
}

/// An audio file's format, as the system reads it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Format {
    /// A four-character code, such as `aac ` or `lpcm`.
    pub format_id: u32,
    pub sample_rate: f64,
    pub channels: u32,
}

pub fn describe(audio: &Path) -> Result<Format, String> {
    let format = AudioFile::open(audio)?.file_format()?;
    Ok(Format {
        format_id: format.mFormatID,
        sample_rate: format.mSampleRate,
        channels: format.mChannelsPerFrame,
    })
}

/// Interleaved 32-bit floats: what Anchovy reads and writes.
fn float_format(sample_rate: f64, channels: u32) -> AudioStreamBasicDescription {
    let bytes = 4 * channels;
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
        mBytesPerPacket: bytes,
        mFramesPerPacket: 1,
        mBytesPerFrame: bytes,
        mChannelsPerFrame: channels,
        mBitsPerChannel: 32,
        mReserved: 0,
    }
}

/// AAC, mono. The encoder fills in the rest.
fn aac_mono(sample_rate: f64) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: sample_rate,
        mFormatID: kAudioFormatMPEG4AAC,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 1,
        mBitsPerChannel: 0,
        mReserved: 0,
    }
}

/// Core Audio errors are four-character codes when printable.
fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 {
        return Ok(());
    }
    let code = status.to_be_bytes();
    let shown = if code.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
        format!("'{}'", String::from_utf8_lossy(&code))
    } else {
        status.to_string()
    };
    Err(format!("macOS could not {what} (error {shown})."))
}

fn url(path: &Path) -> Result<objc2_core_foundation::CFRetained<CFURL>, String> {
    CFURL::from_file_path(path).ok_or_else(|| format!("{} is not a file path.", path.display()))
}

/// An open ExtAudioFile, disposed when dropped.
pub(crate) struct AudioFile(ExtAudioFileRef);

// SAFETY: an ExtAudioFile may be used from any thread, one at a time; this
// type is only ever used by one thread at once.
unsafe impl Send for AudioFile {}

impl AudioFile {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let url = url(path)?;
        let mut file: ExtAudioFileRef = ptr::null_mut();
        // SAFETY: `url` is a valid CFURL and `file` receives the new object.
        let status = unsafe { ExtAudioFileOpenURL(&url, NonNull::from(&mut file)) };
        check(status, "open the audio file")?;
        Ok(AudioFile(file))
    }

    fn create_m4a(path: &Path, format: &AudioStreamBasicDescription) -> Result<Self, String> {
        let url = url(path)?;
        let mut file: ExtAudioFileRef = ptr::null_mut();
        // SAFETY: `url` and `format` are valid for the call; no channel
        // layout is passed for mono.
        let status = unsafe {
            ExtAudioFileCreateWithURL(
                &url,
                kAudioFileM4AType,
                NonNull::from(format),
                ptr::null(),
                AudioFileFlags::EraseFile.0,
                NonNull::from(&mut file),
            )
        };
        check(status, "create the M4A file")?;
        Ok(AudioFile(file))
    }

    fn get<T: Copy + Default>(&self, property: ExtAudioFilePropertyID) -> Result<T, String> {
        let mut value = T::default();
        let mut size = size_of::<T>() as u32;
        // SAFETY: `value` has room for `size` bytes of the property.
        let status = unsafe {
            ExtAudioFileGetProperty(
                self.0,
                property,
                NonNull::from(&mut size),
                NonNull::from(&mut value).cast::<c_void>(),
            )
        };
        check(status, "read the audio file's format")?;
        Ok(value)
    }

    pub(crate) fn file_format(&self) -> Result<AudioStreamBasicDescription, String> {
        let mut format = float_format(0.0, 0);
        let mut size = size_of::<AudioStreamBasicDescription>() as u32;
        // SAFETY: `format` has room for an AudioStreamBasicDescription.
        let status = unsafe {
            ExtAudioFileGetProperty(
                self.0,
                kExtAudioFileProperty_FileDataFormat,
                NonNull::from(&mut size),
                NonNull::from(&mut format).cast::<c_void>(),
            )
        };
        check(status, "read the audio file's format")?;
        Ok(format)
    }

    /// The format reads and writes use; the system converts to and from the
    /// file's own format.
    pub(crate) fn set_client_format(
        &self,
        format: &AudioStreamBasicDescription,
    ) -> Result<(), String> {
        // SAFETY: `format` is a valid description of the given size.
        let status = unsafe {
            ExtAudioFileSetProperty(
                self.0,
                kExtAudioFileProperty_ClientDataFormat,
                size_of::<AudioStreamBasicDescription>() as u32,
                NonNull::from(format).cast::<c_void>(),
            )
        };
        check(status, "set up the audio converter")
    }

    /// Reads up to `samples.len() / channels` frames of interleaved floats.
    /// 0 at the end of the file.
    pub(crate) fn read(&self, samples: &mut [f32], channels: u32) -> Result<usize, String> {
        let mut frames = (samples.len() / channels as usize) as u32;
        let mut list = buffer_list(samples, channels);
        // SAFETY: `list` points at `samples`, which has room for `frames`
        // frames of the client format.
        let status = unsafe {
            ExtAudioFileRead(self.0, NonNull::from(&mut frames), NonNull::from(&mut list))
        };
        check(status, "read the audio")?;
        Ok(frames as usize)
    }

    fn write(&self, samples: &mut [f32], channels: u32) -> Result<(), String> {
        let frames = (samples.len() / channels as usize) as u32;
        let mut list = buffer_list(samples, channels);
        // SAFETY: `list` points at `frames` frames of the client format.
        let status = unsafe { ExtAudioFileWrite(self.0, frames, NonNull::from(&mut list)) };
        check(status, "encode the audio")
    }

    /// Finishes the file and reports whether that worked.
    fn close(self) -> Result<(), String> {
        let file = self.0;
        std::mem::forget(self);
        // SAFETY: `file` is open and is not used again.
        check(unsafe { ExtAudioFileDispose(file) }, "finish the M4A file")
    }
}

impl Drop for AudioFile {
    fn drop(&mut self) {
        // SAFETY: the file is open and is not used again.
        unsafe {
            ExtAudioFileDispose(self.0);
        }
    }
}

fn buffer_list(samples: &mut [f32], channels: u32) -> AudioBufferList {
    AudioBufferList {
        mNumberBuffers: 1,
        mBuffers: [AudioBuffer {
            mNumberChannels: channels,
            mDataByteSize: std::mem::size_of_val(samples) as u32,
            mData: samples.as_mut_ptr().cast(),
        }],
    }
}
