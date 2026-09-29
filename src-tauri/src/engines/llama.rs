//! The llama.cpp engine (`engine: "llama_cpp"`): Qwen3-ASR through mtmd for
//! speech, and chat models for summaries. Every layer runs on the GPU
//! through Metal.
//!
//! These calls need the real model files, so they are exercised by
//! `npm run eval`, not by unit tests. The logic around them lives in
//! `windows.rs`, `summary.rs`, and `pipeline.rs`.

use std::mem::ManuallyDrop;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::gguf::GgufContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use super::summary::ANSWER_TOKENS;
use super::{EngineError, Engines, Heard, ModelFiles, Prompt, Summarizer, Transcriber};

pub const ENGINE: &str = "llama_cpp";

/// Prompt and audio for one 30-second window come to about 420 tokens, and
/// the answer to a few hundred more.
const ASR_CONTEXT: u32 = 4096;
/// A window of speech is never this long as text; a model that gets this far
/// is repeating itself.
const ASR_MAX_TOKENS: usize = 1024;
const BATCH: u32 = 2048;
/// Every layer on the GPU.
const GPU_LAYERS: u32 = 1000;

/// Scratch buffers llama.cpp allocates next to the weights and the context.
const COMPUTE_BYTES: u64 = 512 << 20;
/// Context memory per token when the model's own numbers cannot be read:
/// Qwen3-4B's 36 layers of 8 key and value heads of 128 half floats.
const FALLBACK_KV_BYTES_PER_TOKEN: u64 = 36 * 8 * (128 + 128) * 2;
/// `GGUF_TYPE_UINT32` in ggml's gguf.h. Reading a number of another type
/// aborts inside llama.cpp, so the type is checked first.
const GGUF_TYPE_UINT32: u32 = 4;

/// llama.cpp is set up once per process.
fn backend() -> Result<&'static LlamaBackend, EngineError> {
    static BACKEND: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            // llama.cpp, ggml, and mtmd all log to stderr by default.
            llama_cpp_2::send_logs_to_tracing(
                llama_cpp_2::LogOptions::default().with_logs_enabled(false),
            );
            LlamaBackend::init().map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(|err| EngineError::Load(err.clone()))
}

pub struct LlamaEngines;

fn check_engine(model: &ModelFiles) -> Result<(), EngineError> {
    if model.engine == ENGINE {
        Ok(())
    } else {
        Err(EngineError::UnknownEngine(model.engine.clone()))
    }
}

fn is_projector(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("mmproj"))
}

/// The weights, and the audio projector if the model has one.
fn split_files(model: &ModelFiles) -> Result<(PathBuf, Option<PathBuf>), EngineError> {
    let weights = model
        .files
        .iter()
        .find(|path| !is_projector(path))
        .ok_or_else(|| EngineError::Load(format!("{} has no weights file.", model.id)))?;
    let projector = model.files.iter().find(|path| is_projector(path));
    Ok((weights.clone(), projector.cloned()))
}

fn load_model(path: &Path) -> Result<LlamaModel, EngineError> {
    let params = LlamaModelParams::default().with_n_gpu_layers(GPU_LAYERS);
    LlamaModel::load_from_file(backend()?, path, &params)
        .map_err(|err| EngineError::Load(err.to_string()))
}

impl Engines for LlamaEngines {
    fn transcriber(&self, model: &ModelFiles) -> Result<Box<dyn Transcriber>, EngineError> {
        check_engine(model)?;
        let (weights, projector) = split_files(model)?;
        let projector = projector
            .ok_or_else(|| EngineError::Load(format!("{} has no audio projector.", model.id)))?;
        Ok(Box::new(Asr::load(&weights, &projector)?))
    }

    fn summarizer(
        &self,
        model: &ModelFiles,
        context_tokens: u32,
    ) -> Result<Box<dyn Summarizer>, EngineError> {
        check_engine(model)?;
        let (weights, _) = split_files(model)?;
        Ok(Box::new(Chat::load(&weights, context_tokens)?))
    }

    fn summarizer_bytes(&self, model: &ModelFiles, context_tokens: u32) -> u64 {
        let per_token = split_files(model)
            .ok()
            .and_then(|(weights, _)| kv_bytes_per_token(&weights))
            .unwrap_or(FALLBACK_KV_BYTES_PER_TOKEN);
        model.size_bytes + u64::from(context_tokens) * per_token + COMPUTE_BYTES
    }
}

/// Context memory per token, from the model's header: every layer keeps a
/// key and a value per key-value head, in half floats.
fn kv_bytes_per_token(weights: &Path) -> Option<u64> {
    let gguf = GgufContext::from_file(weights)?;
    let text = |key: &str| {
        let index = gguf.find_key(key);
        (index >= 0).then(|| gguf.val_str(index)).flatten()
    };
    let number = |key: &str| {
        let index = gguf.find_key(key);
        (index >= 0 && gguf.kv_type(index) == GGUF_TYPE_UINT32)
            .then(|| u64::from(gguf.val_u32(index)))
    };
    let arch = text("general.architecture")?.to_string();
    let layers = number(&format!("{arch}.block_count"))?;
    let kv_heads = number(&format!("{arch}.attention.head_count_kv"))?;
    let key = number(&format!("{arch}.attention.key_length")).or_else(|| {
        Some(
            number(&format!("{arch}.embedding_length"))?
                / number(&format!("{arch}.attention.head_count"))?,
        )
    })?;
    let value = number(&format!("{arch}.attention.value_length")).unwrap_or(key);
    Some(layers * kv_heads * (key + value) * 2)
}

/// A model and a context that borrows it. The context is dropped first, then
/// the model, and with them all their memory.
struct Loaded {
    ctx: ManuallyDrop<LlamaContext<'static>>,
    model: *mut LlamaModel,
}

impl Loaded {
    fn new(model: LlamaModel, n_ctx: u32) -> Result<Self, EngineError> {
        let model = Box::into_raw(Box::new(model));
        // SAFETY: the model stays at this address until `drop`, which drops
        // the context before freeing it.
        let model_ref: &'static LlamaModel = unsafe { &*model };
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(n_ctx))
            .with_n_batch(BATCH)
            .with_n_ubatch(BATCH);
        match model_ref.new_context(backend()?, params) {
            Ok(ctx) => Ok(Loaded {
                ctx: ManuallyDrop::new(ctx),
                model,
            }),
            Err(err) => {
                // SAFETY: nothing borrows the model; the context failed.
                drop(unsafe { Box::from_raw(model) });
                Err(EngineError::Load(err.to_string()))
            }
        }
    }

    fn model(&self) -> &LlamaModel {
        // SAFETY: valid until `drop`.
        unsafe { &*self.model }
    }

    /// Samples up to `max` tokens after the prompt already in the context.
    fn generate(
        &mut self,
        mut sampler: LlamaSampler,
        mut position: i32,
        max: usize,
    ) -> Result<String, EngineError> {
        let run = |err: String| EngineError::Run(err);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut batch = LlamaBatch::new(1, 1);
        let mut text = String::new();
        for _ in 0..max {
            let token = sampler.sample(&self.ctx, -1);
            sampler.accept(token);
            if self.model().is_eog_token(token) {
                break;
            }
            text += &self
                .model()
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|err| run(err.to_string()))?;
            batch.clear();
            batch
                .add(token, position, &[0], true)
                .map_err(|err| run(err.to_string()))?;
            self.ctx
                .decode(&mut batch)
                .map_err(|err| run(err.to_string()))?;
            position += 1;
        }
        Ok(text)
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        // SAFETY: the context goes first, then the model it borrowed; neither
        // is used again.
        unsafe {
            ManuallyDrop::drop(&mut self.ctx);
            drop(Box::from_raw(self.model));
        }
    }
}

/// Qwen3-ASR: the text model, the audio projector, and a context.
struct Asr {
    // Declared first, so it is dropped before the model it points into.
    mtmd: MtmdContext,
    loaded: Loaded,
    prompt: String,
}

impl Asr {
    fn load(weights: &Path, projector: &Path) -> Result<Self, EngineError> {
        let loaded = Loaded::new(load_model(weights)?, ASR_CONTEXT)?;
        let params = MtmdContextParams {
            use_gpu: true,
            print_timings: false,
            ..Default::default()
        };
        let projector = projector
            .to_str()
            .ok_or_else(|| EngineError::Load("The projector path is not UTF-8.".into()))?;
        let mtmd = MtmdContext::init_from_file(projector, loaded.model(), &params)
            .map_err(|err| EngineError::Load(err.to_string()))?;
        if !mtmd.support_audio() {
            return Err(EngineError::Load(
                "The projector has no audio encoder.".into(),
            ));
        }
        // Qwen3-ASR's prompt: an empty system turn, then the audio as the
        // user turn. mtmd wraps the marker in <|audio_start|>…<|audio_end|>.
        let marker = params.media_marker.to_string_lossy();
        let prompt = format!(
            "<|im_start|>system\n<|im_end|>\n<|im_start|>user\n{marker}<|im_end|>\n\
             <|im_start|>assistant\n"
        );
        Ok(Asr {
            mtmd,
            loaded,
            prompt,
        })
    }
}

impl Transcriber for Asr {
    fn transcribe(&mut self, samples: &[f32]) -> Result<Heard, EngineError> {
        let run = |err: String| EngineError::Run(err);
        self.loaded.ctx.clear_kv_cache();
        let bitmap = MtmdBitmap::from_audio_data(samples).map_err(|err| run(err.to_string()))?;
        let chunks = self
            .mtmd
            .tokenize(
                MtmdInputText {
                    text: self.prompt.clone(),
                    add_special: false,
                    parse_special: true,
                },
                &[&bitmap],
            )
            .map_err(|err| run(err.to_string()))?;
        let position = chunks
            .eval_chunks(&self.mtmd, &self.loaded.ctx, 0, 0, BATCH as i32, true)
            .map_err(|err| run(err.to_string()))?;
        let raw = self
            .loaded
            .generate(LlamaSampler::greedy(), position, ASR_MAX_TOKENS)?;
        Ok(parse_asr_output(&raw))
    }
}

/// Qwen3-ASR answers `language <Name><asr_text><transcript>`.
pub fn parse_asr_output(raw: &str) -> Heard {
    match raw.split_once("<asr_text>") {
        Some((head, text)) => Heard {
            language: head
                .trim()
                .strip_prefix("language")
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty()),
            text: text.trim().to_string(),
        },
        None => Heard {
            language: None,
            text: raw.trim().to_string(),
        },
    }
}

/// A chat model with its template.
struct Chat {
    loaded: Loaded,
    template: LlamaChatTemplate,
    n_ctx: u32,
}

impl Chat {
    fn load(weights: &Path, n_ctx: u32) -> Result<Self, EngineError> {
        let loaded = Loaded::new(load_model(weights)?, n_ctx)?;
        let template = loaded
            .model()
            .chat_template(None)
            .map_err(|err| EngineError::Load(err.to_string()))?;
        Ok(Chat {
            loaded,
            template,
            n_ctx,
        })
    }

    fn tokens(&self, text: &str) -> Result<Vec<LlamaToken>, EngineError> {
        self.loaded
            .model()
            .str_to_token(text, AddBos::Never)
            .map_err(|err| EngineError::Run(err.to_string()))
    }
}

impl Summarizer for Chat {
    fn count_tokens(&self, text: &str) -> Result<usize, EngineError> {
        Ok(self.tokens(text)?.len())
    }

    fn complete(&mut self, prompt: &Prompt, attempt: u32) -> Result<String, EngineError> {
        let run = |err: String| EngineError::Run(err);
        let messages = [("system", &prompt.system), ("user", &prompt.user)]
            .into_iter()
            .map(|(role, text)| LlamaChatMessage::new(role.into(), text.clone()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| run(err.to_string()))?;
        let text = self
            .loaded
            .model()
            .apply_chat_template(&self.template, &messages, true)
            .map_err(|err| run(err.to_string()))?;
        let tokens = self.tokens(&text)?;
        if tokens.len() + ANSWER_TOKENS > self.n_ctx as usize {
            return Err(run(format!(
                "The request is {} tokens, more than the context holds.",
                tokens.len()
            )));
        }
        self.loaded.ctx.clear_kv_cache();
        let mut batch = LlamaBatch::new(BATCH as usize, 1);
        let last = tokens.len() - 1;
        for (start, block) in tokens.chunks(BATCH as usize).enumerate() {
            batch.clear();
            for (offset, token) in block.iter().enumerate() {
                let position = start * BATCH as usize + offset;
                batch
                    .add(*token, position as i32, &[0], position == last)
                    .map_err(|err| run(err.to_string()))?;
            }
            self.loaded
                .ctx
                .decode(&mut batch)
                .map_err(|err| run(err.to_string()))?;
        }
        // Qwen's recommended settings for its instruct models. A fixed seed
        // per attempt makes a run repeatable; the retry gets another seed.
        let sampler = LlamaSampler::chain_simple([
            LlamaSampler::top_k(20),
            LlamaSampler::top_p(0.8, 1),
            LlamaSampler::temp(0.7),
            LlamaSampler::dist(1 + attempt),
        ]);
        self.loaded
            .generate(sampler, tokens.len() as i32, ANSWER_TOKENS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen3_asr_output_is_split_into_language_and_text() {
        assert_eq!(
            parse_asr_output("language Chinese<asr_text>你好。"),
            Heard {
                language: Some("Chinese".into()),
                text: "你好。".into()
            }
        );
        assert_eq!(
            parse_asr_output(" hello "),
            Heard {
                language: None,
                text: "hello".into()
            }
        );
    }

    #[test]
    fn projector_files_are_told_apart_from_weights() {
        let model = ModelFiles {
            id: "qwen3-asr-1.7b".into(),
            engine: ENGINE.into(),
            display_name: "Qwen3-ASR 1.7B".into(),
            files: vec![
                PathBuf::from("/m/Qwen3-ASR-1.7B-Q8_0.gguf"),
                PathBuf::from("/m/mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"),
            ],
            size_bytes: 1,
        };
        let (weights, projector) = split_files(&model).unwrap();
        assert!(weights.ends_with("Qwen3-ASR-1.7B-Q8_0.gguf"));
        assert!(projector
            .unwrap()
            .ends_with("mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"));
    }

    #[test]
    fn other_engines_are_refused() {
        let model = ModelFiles {
            id: "x".into(),
            engine: "whisper".into(),
            display_name: "X".into(),
            files: vec![],
            size_bytes: 0,
        };
        let err = LlamaEngines.transcriber(&model).err().unwrap();
        assert_eq!(err, EngineError::UnknownEngine("whisper".into()));
    }

    #[test]
    fn summary_memory_falls_back_to_qwen3_4b_numbers_without_a_readable_header() {
        let model = ModelFiles {
            id: "x".into(),
            engine: ENGINE.into(),
            display_name: "X".into(),
            files: vec![PathBuf::from("/does/not/exist.gguf")],
            size_bytes: 2_497_281_120,
        };
        assert_eq!(
            LlamaEngines.summarizer_bytes(&model, 27_072),
            2_497_281_120 + 27_072 * 147_456 + (512 << 20)
        );
    }
}
