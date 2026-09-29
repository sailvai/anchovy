// `npm run eval`: model quality evaluation on a real Mac with the shipped
// default models. Never part of `npm run verify` or CI.
//
// For each sample in evals/samples/, macOS's built-in voice reads
// transcript.txt into a 48 kHz WAV file (the app's recording format), and the
// app's own pipeline (src-tauri/examples/eval.rs) turns it into a note. Each
// sample runs twice: once as the app would, and once with small summary
// chunks so the chunk-and-merge path runs on the real model too.
//
// Pass: Chinese character error and English word error within 2 points of
// evals/baseline.json, every summary valid JSON, every decision and action
// item supported by the transcript, and every summary in the language spoken.
// The first passing run writes the baseline.
//
//   npm run eval                 run every sample
//   npm run eval -- --fetch      download the default models first
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  compareWithBaseline,
  covered,
  errorRate,
  languageOf,
  percent,
  support,
  SUPPORT_THRESHOLD,
} from "./eval-score.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(root, "src-tauri");
const samplesDir = path.join(root, "evals", "samples");
const baselinePath = path.join(root, "evals", "baseline.json");
const clipsDir = path.join(root, "node_modules", ".tmp", "eval-clips");
const runner = path.join(tauriDir, "target", "release", "examples", "eval");
// Small enough that every sample is summarized in two or more chunks.
const SMALL_CHUNK_TOKENS = 120;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1 << 26, ...options });
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
  return result.stdout;
}

function machine() {
  const sysctl = (name) => run("sysctl", ["-n", name]).trim();
  return {
    chip: sysctl("machdep.cpu.brand_string"),
    memory_gb: Number(sysctl("hw.memsize")) / 2 ** 30,
    macos: run("sw_vers", ["-productVersion"]).trim(),
  };
}

function loadSamples() {
  return readdirSync(samplesDir)
    .filter((id) => existsSync(path.join(samplesDir, id, "sample.json")))
    .sort()
    .map((id) => ({
      id,
      ...JSON.parse(readFileSync(path.join(samplesDir, id, "sample.json"), "utf8")),
      transcript: readFileSync(path.join(samplesDir, id, "transcript.txt"), "utf8").trim(),
    }));
}

// The clips are made on this Mac and never committed.
function makeClip(sample) {
  mkdirSync(clipsDir, { recursive: true });
  const aiff = path.join(clipsDir, `${sample.id}.aiff`);
  const wav = path.join(clipsDir, `${sample.id}.wav`);
  run("say", [
    "-v",
    sample.voice,
    "-o",
    aiff,
    "-f",
    path.join(samplesDir, sample.id, "transcript.txt"),
  ]);
  run("afconvert", ["-f", "WAVE", "-d", "LEI16@48000", "-c", "1", aiff, wav]);
  return wav;
}

function score(sample, output, { scoreTranscript }) {
  const result = { id: sample.id, language: sample.language, ok: output.ok, problems: [] };
  if (!output.ok) {
    result.problems.push(`No note: ${output.error}`);
    return result;
  }
  const report = output.report;
  const heard = report.segments.map((segment) => segment.text).join(" ");
  if (scoreTranscript) result.transcription = errorRate(sample.transcript, heard, sample.language);

  // The pipeline only writes a note after a valid answer; count how many
  // answers were needed.
  result.answers = output.answers.length;
  result.retries = output.answers.filter((answer) => answer.attempt > 0).length;

  const items = [
    ...report.summary.decisions.map((text) => ({ kind: "decision", text })),
    ...report.summary.action_items.map((text) => ({ kind: "action item", text })),
  ];
  result.items = items.map((item) => ({
    ...item,
    support: support(item.text, sample.transcript, sample.language),
  }));
  for (const item of result.items) {
    if (item.support < SUPPORT_THRESHOLD) {
      result.problems.push(
        `Unsupported ${item.kind} (${percent(item.support)} said): ${item.text}`,
      );
    }
  }
  const written = [report.summary.summary, ...items.map((item) => item.text)].join(" ");
  result.summaryLanguage = languageOf(written);
  if (result.summaryLanguage !== sample.language) {
    result.problems.push(
      `Summary is in ${result.summaryLanguage}, speech is in ${sample.language}.`,
    );
  }
  result.coveredDecisions = covered(sample.decisions, report.summary.decisions, sample.language);
  result.coveredActions = covered(
    sample.action_items,
    report.summary.action_items,
    sample.language,
  );
  result.checked = { decisions: sample.decisions.length, action_items: sample.action_items.length };
  result.seconds = {
    audio: report.audio_seconds,
    transcribe: report.transcribe_seconds,
    summarize: report.summarize_seconds,
  };
  result.memory = {
    peak_footprint: output.peak_footprint,
    footprint_before: report.footprint_before,
    footprint_after_unload: report.footprint_after_unload,
  };
  return result;
}

function rate(results, language) {
  const scored = results.filter((r) => r.language === language && r.transcription);
  const total = scored.reduce((sum, r) => sum + r.transcription.length, 0);
  const wrong = scored.reduce((sum, r) => sum + r.transcription.edits, 0);
  return total ? wrong / total : null;
}

const mb = (bytes) => (bytes == null ? "-" : `${(bytes / 1e6).toFixed(0)} MB`);

function main() {
  const args = process.argv.slice(2);
  console.log("=== Building the eval runner (release)");
  run("cargo", ["build", "--release", "--example", "eval"], { cwd: tauriDir, stdio: "inherit" });
  if (args.includes("--fetch")) {
    run(runner, ["--fetch"], { stdio: "inherit" });
  }

  const samples = loadSamples();
  const results = [];
  for (const sample of samples) {
    const clip = makeClip(sample);
    for (const variant of ["app", "chunked"]) {
      const extra = variant === "chunked" ? ["--chunk-tokens", String(SMALL_CHUNK_TOKENS)] : [];
      console.log(`=== ${sample.id} (${variant})`);
      const output = JSON.parse(run(runner, [clip, ...extra]));
      const result = score(sample, output, { scoreTranscript: variant === "app" });
      result.variant = variant;
      result.note = output.note;
      results.push(result);
    }
  }

  const current = { chinese_cer: rate(results, "zh"), english_wer: rate(results, "en") };
  console.log("\n=== Samples");
  for (const r of results) {
    const t = r.transcription;
    const errorLabel = r.language === "zh" ? "CER" : "WER";
    console.log(
      [
        `${r.id} (${r.variant})`,
        t ? `${errorLabel} ${percent(t.edits / t.length)} (${t.edits}/${t.length})` : null,
        r.ok
          ? `JSON valid, ${r.answers} answer(s), ${r.retries} retr${r.retries === 1 ? "y" : "ies"}`
          : "no note",
        r.items
          ? `${r.items.length} items, lowest support ${percent(Math.min(1, ...r.items.map((i) => i.support)))}`
          : null,
        r.summaryLanguage ? `summary ${r.summaryLanguage}` : null,
        r.coveredDecisions != null
          ? `covers ${r.coveredDecisions}/${r.checked.decisions} decisions, ${r.coveredActions}/${r.checked.action_items} action items`
          : null,
        r.seconds
          ? `${r.seconds.audio.toFixed(0)} s audio: transcribe ${r.seconds.transcribe.toFixed(1)} s, summarize ${r.seconds.summarize.toFixed(1)} s`
          : null,
        r.memory
          ? `peak ${mb(r.memory.peak_footprint)}, footprint ${mb(r.memory.footprint_before)} before ASR, ${mb(r.memory.footprint_after_unload)} after unload`
          : null,
      ]
        .filter(Boolean)
        .join(" | "),
    );
    for (const problem of r.problems) console.log(`  FAIL ${problem}`);
  }

  const failures = results.flatMap((r) => r.problems.map((p) => `${r.id} (${r.variant}): ${p}`));
  console.log("\n=== Totals");
  console.log(`Chinese character error rate: ${percent(current.chinese_cer)}`);
  console.log(`English word error rate:      ${percent(current.english_wer)}`);

  const env = machine();
  if (existsSync(baselinePath)) {
    const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
    console.log(
      `Baseline (${baseline.created}, ${baseline.machine.chip}): CER ${percent(baseline.chinese_cer)}, WER ${percent(baseline.english_wer)}`,
    );
    failures.push(...compareWithBaseline(current, baseline));
  } else if (failures.length === 0) {
    const baseline = {
      created: new Date().toISOString().slice(0, 10),
      machine: env,
      models: {
        transcribe: results.find((r) => r.ok)?.note?.match(/^asr_model: (.*)$/m)?.[1] ?? null,
        summarize: results.find((r) => r.ok)?.note?.match(/^summary_model: (.*)$/m)?.[1] ?? null,
      },
      chinese_cer: current.chinese_cer,
      english_wer: current.english_wer,
      samples: Object.fromEntries(
        results
          .filter((r) => r.variant === "app")
          .map((r) => [
            r.id,
            {
              error_rate: r.transcription.edits / r.transcription.length,
              seconds: r.seconds,
              memory: r.memory,
            },
          ]),
      ),
    };
    writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
    console.log(`First passing run: wrote ${path.relative(root, baselinePath)}.`);
  }

  console.log(`\nMachine: ${env.chip}, ${env.memory_gb} GB, macOS ${env.macos}.`);
  if (failures.length) {
    console.log(`eval FAILED (${failures.length}):`);
    for (const failure of failures) console.log(`  ${failure}`);
    process.exit(1);
  }
  console.log("eval passed");
}

main();
