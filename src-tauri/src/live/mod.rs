//! The live transcript: while recording, the words just spoken, shown in the
//! window phrase by phrase.
//!
//! `segmenter` finds where each phrase starts and ends in 16 kHz mono
//! samples. Each finished phrase is transcribed whole and shown once; it is
//! never changed afterwards. Nothing here writes a file or a log.

pub mod segmenter;

/// One finished phrase, ready for the transcription model. Positions count
/// samples from the moment the segmenter was created.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// The first sample in `samples`.
    pub start: usize,
    /// Just past the last sample of the last frame that held sound: where
    /// the speaker stopped.
    pub voice_end: usize,
    /// Just past the last sample fed in when the phrase was judged finished.
    pub cut: usize,
    /// The audio sent to the model: the phrase with a little silence on
    /// either side.
    pub samples: Vec<f32>,
}
