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
// item supported by the transcript and none of them a sample's not-items
// (things said that were never agreed or taken on), every summary in the
// language spoken, no
// words lost or repeated where transcription windows join, and the small-chunk
// run really chunked and merged. A sample in both languages keeps its error
// rate within 2 points of its baseline. For the hour-long sample,
// transcription time, summary time, and peak memory within 20% of the
// baseline. The first passing run writes the baseline; a sample new to it is
// added on its first pass. In a mixed sample an unsupported item is a
// warning, not a failure: the model translates items between the two
// languages, and a word-overlap check cannot see a faithful translation.
//
// Small quality (plan step 8): one short sample runs again as an M4A that
// afconvert makes from its WAV at eval time. Its error rate is shown next to
// the WAV run and must be within 1 point of it. It is not counted in the
// totals and never goes into the baseline, and the audio is never committed.
//
//   npm run eval                      run every sample
//   npm run eval -- --sample <id>     run one sample (repeatable)
//   npm run eval -- --fetch           download the default models first
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { makeClip, parseScript } from "./eval-audio.mjs";
import {
  checkJoins,
  compareCost,
  compareSampleError,
  compareWithBaseline,
  covered,
  errorRate,
  JOIN_LIMIT,
  judgeItems,
  languageOf,
  megabytes as mb,
  percent,
  summaryCalls,
} from "./eval-score.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = path.join(root, "src-tauri");
const samplesDir = path.join(root, "evals", "samples");
const baselinePath = path.join(root, "evals", "baseline.json");
const clipsDir = path.join(root, "node_modules", ".tmp", "eval-clips");
const runner = path.join(tauriDir, "target", "release", "examples", "eval");
// Small enough that every short sample is summarized in two or more chunks.
// A long sample sets its own size in sample.json.
const SMALL_CHUNK_TOKENS = 120;
// The sample that also runs as an M4A, and how far its error rate may be from
// the WAV run's.
const M4A_SAMPLE = "en-planning";
const M4A_LIMIT = 0.01;

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

// A sample is transcript.txt read by one voice, or script.txt read turn by
// turn by the voice named on each line.
function loadSample(id) {
  const dir = path.join(samplesDir, id);
  const sample = { id, ...JSON.parse(readFileSync(path.join(dir, "sample.json"), "utf8")) };
  if (existsSync(path.join(dir, "script.txt"))) {
    sample.turns = parseScript(readFileSync(path.join(dir, "script.txt"), "utf8"));
    sample.transcript = sample.turns.map((turn) => turn.text).join("\n");
  } else {
    sample.transcript = readFileSync(path.join(dir, "transcript.txt"), "utf8").trim();
  }
  return sample;
}

function loadSamples(only) {
  const ids = readdirSync(samplesDir)
    .filter((id) => existsSync(path.join(samplesDir, id, "sample.json")))
    .sort();
  for (const id of only) if (!ids.includes(id)) throw new Error(`No sample ${id}.`);
  return ids.filter((id) => only.length === 0 || only.includes(id)).map(loadSample);
}

function score(sample, output, { scoreTranscript, mustChunk }) {
  const result = {
    id: sample.id,
    language: sample.language,
    ok: output.ok,
    problems: [],
    warnings: [],
  };
  if (!output.ok) {
    result.problems.push(`No note: ${output.error}`);
    return result;
  }
  const report = output.report;
  const heard = report.segments.map((segment) => segment.text).join(" ");
  if (scoreTranscript) {
    result.transcription = errorRate(sample.transcript, heard, sample.language);
    result.joins = checkJoins(
      sample.transcript,
      report.segments,
      output.heard.map(([, text]) => text),
      sample.language,
    );
    for (const join of result.joins) {
      if (join.lost >= JOIN_LIMIT || join.repeated >= JOIN_LIMIT) {
        result.problems.push(
          `At the window join at ${clock(join.start_seconds)}, ${join.lost} unit(s) lost and ${join.repeated} repeated.`,
        );
      }
    }
  }

  // The pipeline only writes a note after a valid answer; count how many
  // answers were needed.
  result.answers = output.answers.length;
  result.retries = output.answers.filter((answer) => answer.attempt > 0).length;
  result.calls = summaryCalls(output.answers);
  if (mustChunk && !(result.calls.chunks > 1 && result.calls.merges > 0)) {
    result.problems.push(
      `Summary was not chunked and merged: ${result.calls.chunks} chunk(s), ${result.calls.merges} merge(s) at ${output.chunk_tokens} tokens per chunk.`,
    );
  }

  const items = [
    ...report.summary.decisions.map((text) => ({ kind: "decision", text })),
    ...report.summary.action_items.map((text) => ({ kind: "action item", text })),
  ];
  const judged = judgeItems(items, sample);
  result.items = judged.items;
  result.problems.push(...judged.problems);
  result.warnings.push(...judged.warnings);
  const written = [report.summary.summary, ...items.map((item) => item.text)].join(" ");
  result.summaryLanguage = languageOf(written);
  // A meeting in both languages may be summarized in either (plan step 6b).
  const spoken = sample.language === "mixed" ? ["zh", "en"] : [sample.language];
  if (!spoken.includes(result.summaryLanguage)) {
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

function clock(seconds) {
  const whole = Math.floor(seconds);
  return [whole / 3600, (whole / 60) % 60, whole % 60]
    .map((n) => String(Math.floor(n)).padStart(2, "0"))
    .join(":");
}

function sampleBaseline(r) {
  return {
    error_rate: r.transcription.edits / r.transcription.length,
    seconds: r.seconds,
    memory: r.memory,
  };
}

// The M4A run: afconvert makes AAC, mono, at the clip's 48 kHz, as Small
// does. Kept out of `results`, so it never reaches the totals or the baseline.
function runM4a(sample, clip, wavRun) {
  if (!sample || !clip || !wavRun?.transcription) return null;
  const m4aClip = clip.replace(/\.wav$/, ".m4a");
  console.log(`=== ${sample.id} (m4a): afconvert to AAC, then the same pipeline`);
  run("afconvert", ["-f", "m4af", "-d", "aac", "-c", "1", clip, m4aClip]);
  const stdout = run(runner, [m4aClip]);
  writeFileSync(path.join(clipsDir, `${sample.id}-m4a.json`), stdout);
  const output = JSON.parse(stdout);
  const result = score(sample, output, { scoreTranscript: true, mustChunk: false });
  const wavRate = wavRun.transcription.edits / wavRun.transcription.length;
  const m4aRate = result.transcription
    ? result.transcription.edits / result.transcription.length
    : null;
  const problems = [...result.problems];
  if (m4aRate != null && Math.abs(m4aRate - wavRate) > M4A_LIMIT) {
    problems.push(
      `Error rate ${percent(m4aRate)} as M4A is more than ${M4A_LIMIT * 100} point from ${percent(wavRate)} as WAV.`,
    );
  }
  return { wavRate, m4aRate, problems };
}

function main() {
  const args = process.argv.slice(2);
  console.log("=== Building the eval runner (release)");
  run("cargo", ["build", "--release", "--example", "eval"], { cwd: tauriDir, stdio: "inherit" });
  if (args.includes("--fetch")) {
    run(runner, ["--fetch"], { stdio: "inherit" });
  }

  const only = args.flatMap((arg, i) => (args[i - 1] === "--sample" ? [arg] : []));
  const samples = loadSamples(only);
  const results = [];
  const clips = {};
  for (const sample of samples) {
    console.log(`=== ${sample.id}: making the audio`);
    const clip = makeClip(sample, path.join(samplesDir, sample.id), clipsDir);
    clips[sample.id] = clip;
    for (const variant of ["app", "chunked"]) {
      const chunkTokens = sample.summary_chunk_tokens ?? SMALL_CHUNK_TOKENS;
      const extra = variant === "chunked" ? ["--chunk-tokens", String(chunkTokens)] : [];
      console.log(`=== ${sample.id} (${variant})`);
      const stdout = run(runner, [clip, ...extra]);
      // Kept next to the clip for a closer look; never committed.
      writeFileSync(path.join(clipsDir, `${sample.id}-${variant}.json`), stdout);
      const output = JSON.parse(stdout);
      const result = score(sample, output, {
        scoreTranscript: variant === "app",
        mustChunk: variant === "chunked",
      });
      result.variant = variant;
      result.timed = sample.timed === true && variant === "app";
      result.note = output.note;
      results.push(result);
    }
  }

  const m4a = runM4a(
    samples.find((sample) => sample.id === M4A_SAMPLE),
    clips[M4A_SAMPLE],
    results.find((r) => r.id === M4A_SAMPLE && r.variant === "app"),
  );

  const current = { chinese_cer: rate(results, "zh"), english_wer: rate(results, "en") };
  console.log("\n=== Samples");
  for (const r of results) {
    const t = r.transcription;
    const errorLabel = { zh: "CER", en: "WER", mixed: "error (characters and words)" }[r.language];
    const joinTrouble = r.joins?.filter((j) => j.lost || j.repeated) ?? [];
    console.log(
      [
        `${r.id} (${r.variant})`,
        t ? `${errorLabel} ${percent(t.edits / t.length)} (${t.edits}/${t.length})` : null,
        r.joins
          ? `${r.joins.length} joins, ${joinTrouble.length} with a unit lost or repeated (most ${Math.max(0, ...r.joins.map((j) => Math.max(j.lost, j.repeated)))})`
          : null,
        r.ok
          ? `JSON valid, ${r.answers} answer(s), ${r.retries} retr${r.retries === 1 ? "y" : "ies"}, ${r.calls.chunks} chunk(s), ${r.calls.merges} merge(s)`
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
    for (const warning of r.warnings) console.log(`  WARN ${warning}`);
  }

  const failures = results.flatMap((r) => r.problems.map((p) => `${r.id} (${r.variant}): ${p}`));
  if (m4a) {
    console.log(
      `${M4A_SAMPLE} (m4a) | error ${m4a.m4aRate == null ? "none" : percent(m4a.m4aRate)} as M4A, ${percent(m4a.wavRate)} as WAV | difference ${m4a.m4aRate == null ? "n/a" : `${((m4a.m4aRate - m4a.wavRate) * 100).toFixed(1)} points`} (limit ${M4A_LIMIT * 100})`,
    );
    for (const problem of m4a.problems) console.log(`  FAIL ${problem}`);
    failures.push(...m4a.problems.map((p) => `${M4A_SAMPLE} (m4a): ${p}`));
  } else {
    console.log(
      `${M4A_SAMPLE} (m4a) | not run: the WAV run of ${M4A_SAMPLE} did not run or failed`,
    );
  }
  const warnings = results.flatMap((r) => r.warnings.map((w) => `${r.id} (${r.variant}): ${w}`));
  console.log("\n=== Totals");
  console.log(
    `Chinese character error rate: ${current.chinese_cer == null ? "no sample run" : percent(current.chinese_cer)}`,
  );
  console.log(
    `English word error rate:      ${current.english_wer == null ? "no sample run" : percent(current.english_wer)}`,
  );
  console.log(
    `Warnings: ${warnings.length} unsupported item(s) in mixed samples (known mixed-language limit, not failures)`,
  );
  for (const warning of warnings) console.log(`  ${warning}`);

  const env = machine();
  const passed = results.filter((r) => r.variant === "app" && r.ok);
  if (existsSync(baselinePath)) {
    const baseline = JSON.parse(readFileSync(baselinePath, "utf8"));
    console.log(
      `Baseline (${baseline.created}, ${baseline.machine.chip}): CER ${percent(baseline.chinese_cer)}, WER ${percent(baseline.english_wer)}`,
    );
    failures.push(...compareWithBaseline(current, baseline));
    for (const r of results.filter((r) => r.timed && r.ok && baseline.samples[r.id])) {
      failures.push(...compareCost(r.id, r, baseline.samples[r.id]));
    }
    for (const r of results.filter(
      (r) => r.language === "mixed" && r.transcription && baseline.samples[r.id],
    )) {
      const rate = r.transcription.edits / r.transcription.length;
      failures.push(...compareSampleError(r.id, rate, baseline.samples[r.id]));
    }
    const added = passed.filter((r) => !baseline.samples[r.id]);
    if (failures.length === 0 && added.length) {
      for (const r of added) baseline.samples[r.id] = sampleBaseline(r);
      baseline.samples = Object.fromEntries(Object.entries(baseline.samples).sort());
      writeFileSync(baselinePath, `${JSON.stringify(baseline, null, 2)}\n`);
      console.log(
        `Added ${added.map((r) => r.id).join(", ")} to ${path.relative(root, baselinePath)}.`,
      );
    }
  } else if (failures.length === 0 && only.length === 0) {
    const baseline = {
      created: new Date().toISOString().slice(0, 10),
      machine: env,
      models: {
        transcribe: results.find((r) => r.ok)?.note?.match(/^asr_model: (.*)$/m)?.[1] ?? null,
        summarize: results.find((r) => r.ok)?.note?.match(/^summary_model: (.*)$/m)?.[1] ?? null,
      },
      chinese_cer: current.chinese_cer,
      english_wer: current.english_wer,
      samples: Object.fromEntries(passed.map((r) => [r.id, sampleBaseline(r)])),
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
