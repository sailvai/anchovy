//! Running the models: speech to text and text to a summary.
//!
//! Code outside this folder depends only on the [`Transcriber`] and
//! [`Summarizer`] traits and on [`Engines`], which loads them. `llama.rs` is
//! the one engine today (`engine: "llama_cpp"` in the shipped list). The rest
//! of this folder is engine-independent logic: `audio` turns a recording into
//! 16 kHz mono samples, `windows` splits them and joins the text again, and
//! `summary` asks for the JSON the note is built from and checks it.

pub mod audio;
pub mod llama;
pub mod mac;
pub mod summary;
pub mod windows;

use std::fmt;
use std::path::PathBuf;

/// What a transcription model heard in one window of audio.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Heard {
    /// The language the model reported, for example "Chinese", if any.
    pub language: Option<String>,
    pub text: String,
}

/// Speech to text. Loaded once per note and dropped before the summary model
/// is loaded, so the two are never in memory together.
pub trait Transcriber {
    /// Transcribes one window of 16 kHz mono samples.
    fn transcribe(&mut self, samples: &[f32]) -> Result<Heard, EngineError>;
}

/// One chat request: a system prompt and the user turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub system: String,
    pub user: String,
}

/// Text to text. `summary.rs` builds the prompts and checks the answers.
pub trait Summarizer {
    /// Tokens `text` takes in this model, to size the chunks.
    fn count_tokens(&self, text: &str) -> Result<usize, EngineError>;
    /// The model's answer. `attempt` starts at 0; a retry passes 1, so an
    /// engine that samples can answer differently.
    fn complete(&mut self, prompt: &Prompt, attempt: u32) -> Result<String, EngineError>;
}

/// A model from the shipped list, ready to load: its name and its files on
/// this Mac, in the order the list gives them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFiles {
    pub id: String,
    pub engine: String,
    pub display_name: String,
    pub files: Vec<PathBuf>,
    /// Total size of the files in bytes.
    pub size_bytes: u64,
}

/// Loads models. The pipeline drops what it gets back to unload it.
pub trait Engines: Send + Sync {
    fn transcriber(&self, model: &ModelFiles) -> Result<Box<dyn Transcriber>, EngineError>;
    /// `context_tokens` is the longest prompt plus answer the summarizer must
    /// hold.
    fn summarizer(
        &self,
        model: &ModelFiles,
        context_tokens: u32,
    ) -> Result<Box<dyn Summarizer>, EngineError>;
    /// About how much memory the summarizer takes with `context_tokens`:
    /// its weights and its context.
    fn summarizer_bytes(&self, model: &ModelFiles, context_tokens: u32) -> u64;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The shipped list names an engine this build does not have.
    UnknownEngine(String),
    /// The model files could not be loaded.
    Load(String),
    /// The model failed while running.
    Run(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::UnknownEngine(engine) => {
                write!(f, "Anchovy has no engine named {engine}.")
            }
            EngineError::Load(err) => write!(f, "The model could not be loaded. {err}"),
            EngineError::Run(err) => write!(f, "The model stopped with an error. {err}"),
        }
    }
}

impl std::error::Error for EngineError {}

/// Page counts from the kernel's VM statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmPages {
    pub page_size: u64,
    pub wired: u64,
    pub compressed: u64,
    /// Pages apps allocated, as opposed to cached files.
    pub anonymous: u64,
    /// Anonymous pages apps marked as safe to discard.
    pub purgeable: u64,
}

/// Memory a new model could get without swapping: everything except what
/// is wired, compressed, or held by apps. Cached files and purgeable memory
/// count as available, as Activity Monitor counts them.
pub fn available_bytes(total: u64, pages: &VmPages) -> u64 {
    let held = pages.wired + pages.compressed + pages.anonymous.saturating_sub(pages.purgeable);
    total.saturating_sub(held * pages.page_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_files_and_purgeable_memory_count_as_available() {
        let pages = VmPages {
            page_size: 16_384,
            wired: 100_000,
            compressed: 50_000,
            anonymous: 400_000,
            purgeable: 20_000,
        };
        let held = (100_000 + 50_000 + 380_000) * 16_384;
        assert_eq!(available_bytes(24 << 30, &pages), (24 << 30) - held);
        assert_eq!(available_bytes(1 << 30, &pages), 0);
    }
}
