//! Plan step 6a spike: runs Qwen3-ASR 1.7B from the shipped list through
//! llama-cpp-2 with mtmd on one clip and prints what it heard and how long it
//! took. Downloads go through the app's own model store.

use anchovy_lib::models::catalog::{Catalog, Model};
use anchovy_lib::models::{download, mac, store::Store};
use asr_spike::{error_rate, hour_estimate, parse_output, read_wav, sha256_file, units};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::mtmd::{MtmdBitmap, MtmdContext, MtmdContextParams, MtmdInputText};
use llama_cpp_2::sampling::LlamaSampler;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use std::{env, fs, process};

const MODEL_ID: &str = "qwen3-asr-1.7b";
const MAX_NEW_TOKENS: usize = 1024;

fn usage() -> ! {
    eprintln!(
        "usage: asr-spike fetch [model-id]\n       \
         asr-spike run <clip.wav> [--script <text file>] [--words] [--model <model-id>]"
    );
    process::exit(2)
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("fetch") => fetch(args.get(1).map_or(MODEL_ID, String::as_str)),
        Some("run") => run(&args[1..]),
        _ => usage(),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        process::exit(1);
    }
}

fn model_and_store(id: &str) -> Result<(Model, Store), String> {
    let catalog = Catalog::shipped();
    let model = catalog
        .get(id)
        .ok_or(format!("{id} is not in the shipped list"))?
        .clone();
    let root = mac::models_dir().map_err(|err| err.to_string())?;
    Ok((model, Store::new(root)))
}

fn fetch(id: &str) -> Result<(), String> {
    let (model, store) = model_and_store(id)?;
    if store.is_usable(&model) {
        println!("{id} is already in {}", store.model_dir(&model).display());
        return Ok(());
    }
    let client = download::client();
    let mut last = u64::MAX;
    download::download_model(&client, &store, &model, &AtomicBool::new(false), &mut |p| {
        let percent = p.downloaded * 100 / p.total.max(1);
        if percent != last {
            last = percent;
            eprint!("\r{percent}%");
        }
    })
    .map_err(|err| err.to_string())?;
    eprintln!();
    println!("{id} downloaded to {}", store.model_dir(&model).display());
    Ok(())
}

/// Hashes every file again, even though the download checked it, so the run
/// never uses a file that changed on disk afterwards.
fn verified_files(model: &Model, store: &Store) -> Result<Vec<PathBuf>, String> {
    if !store.is_usable(model) {
        return Err(format!("{} is not downloaded; run `fetch` first", model.id));
    }
    let dir = store.model_dir(model);
    model
        .files
        .iter()
        .map(|file| {
            let path = dir.join(&file.name);
            let hash = sha256_file(&path).map_err(|err| format!("{}: {err}", file.name))?;
            if hash != file.sha256 {
                return Err(format!(
                    "{}: sha256 {hash}, expected {}",
                    file.name, file.sha256
                ));
            }
            eprintln!("sha256 ok  {}", file.name);
            Ok(path)
        })
        .collect()
}

fn run(args: &[String]) -> Result<(), String> {
    let clip = args.first().unwrap_or_else(|| usage());
    let mut script = None;
    let mut by_word = false;
    let mut id = MODEL_ID.to_string();
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--script" => script = Some(rest.next().unwrap_or_else(|| usage()).clone()),
            "--words" => by_word = true,
            "--model" => id = rest.next().unwrap_or_else(|| usage()).clone(),
            _ => usage(),
        }
    }

    let (model_entry, store) = model_and_store(&id)?;
    let paths = verified_files(&model_entry, &store)?;
    let (weights, mmproj) = match paths.as_slice() {
        [w, m]
            if m.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("mmproj") =>
        {
            (w, m)
        }
        _ => return Err("expected the model file, then its mmproj".into()),
    };

    let bytes = fs::read(clip).map_err(|err| format!("{clip}: {err}"))?;
    let audio = read_wav(&bytes)?;
    let audio_secs = audio.len() as f64 / f64::from(asr_spike::SAMPLE_RATE);

    let started = Instant::now();
    let backend = LlamaBackend::init().map_err(|err| err.to_string())?;
    // Every layer on the GPU (Metal).
    let model_params = LlamaModelParams::default().with_n_gpu_layers(1000);
    let model = LlamaModel::load_from_file(&backend, weights, &model_params)
        .map_err(|err| err.to_string())?;
    let mtmd_params = MtmdContextParams {
        use_gpu: true,
        print_timings: false,
        ..Default::default()
    };
    let mtmd = MtmdContext::init_from_file(&mmproj.to_string_lossy(), &model, &mtmd_params)
        .map_err(|err| err.to_string())?;
    if !mtmd.support_audio() {
        return Err("the mmproj has no audio encoder".into());
    }
    let n_batch = 2048;
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(8192))
        .with_n_batch(n_batch)
        .with_n_ubatch(n_batch);
    let mut ctx = model
        .new_context(&backend, ctx_params)
        .map_err(|err| err.to_string())?;
    let load_secs = started.elapsed().as_secs_f64();

    // Qwen3-ASR's prompt: an empty system turn, then the audio as the user
    // turn. mtmd wraps the marker in <|audio_start|> and <|audio_end|>.
    let marker = mtmd_params.media_marker.to_string_lossy().into_owned();
    let prompt = format!(
        "<|im_start|>system\n<|im_end|>\n<|im_start|>user\n{marker}<|im_end|>\n\
         <|im_start|>assistant\n"
    );
    let bitmap = MtmdBitmap::from_audio_data(&audio).map_err(|err| err.to_string())?;

    let started = Instant::now();
    let chunks = mtmd
        .tokenize(
            MtmdInputText {
                text: prompt,
                add_special: false,
                parse_special: true,
            },
            &[&bitmap],
        )
        .map_err(|err| err.to_string())?;
    let mut n_past = chunks
        .eval_chunks(&mtmd, &ctx, 0, 0, n_batch as i32, true)
        .map_err(|err| err.to_string())?;
    let prefill_secs = started.elapsed().as_secs_f64();

    let started = Instant::now();
    let mut sampler = LlamaSampler::greedy();
    let mut decoder = encoding_rs::UTF_8.new_decoder();
    let mut batch = LlamaBatch::new(1, 1);
    let mut raw = String::new();
    let mut generated = 0;
    while generated < MAX_NEW_TOKENS {
        let token = sampler.sample(&ctx, -1);
        sampler.accept(token);
        if model.is_eog_token(token) {
            break;
        }
        raw += &model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|err| err.to_string())?;
        generated += 1;
        batch.clear();
        batch
            .add(token, n_past, &[0], true)
            .map_err(|err| err.to_string())?;
        ctx.decode(&mut batch).map_err(|err| err.to_string())?;
        n_past += 1;
    }
    let decode_secs = started.elapsed().as_secs_f64();

    let output = parse_output(&raw);
    let work_secs = prefill_secs + decode_secs;
    println!("clip          {clip}");
    println!("model         {id}");
    println!(
        "audio         {audio_secs:.1} s, {} audio+prompt tokens",
        chunks.total_tokens()
    );
    println!("load          {load_secs:.2} s");
    println!("encode+prefill {prefill_secs:.2} s");
    println!("decode        {decode_secs:.2} s, {generated} tokens");
    println!(
        "real-time     {:.1}x faster than real time",
        audio_secs / work_secs
    );
    println!(
        "one hour      ~{:.0} s if time scales with length",
        hour_estimate(load_secs, work_secs, audio_secs)
    );
    println!(
        "language      {}",
        output.language.as_deref().unwrap_or("-")
    );
    if let Some(script) = script {
        let reference = fs::read_to_string(&script).map_err(|err| format!("{script}: {err}"))?;
        let rate = error_rate(&units(&reference, by_word), &units(&output.text, by_word));
        println!(
            "{}           {:.1}%",
            if by_word { "WER" } else { "CER" },
            rate * 100.0
        );
    }
    println!("transcript    {}", output.text);
    Ok(())
}
