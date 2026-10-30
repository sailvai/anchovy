// `npm run verify:live`: the live transcript's 5-second check on a real Mac
// with the transcription model selected in Models. Never part of
// `npm run verify` or CI.
//
// macOS's built-in voices read scripts/live-sentences.json, our own
// sentences: Chinese with Tingting, English with Samantha. Each is trimmed
// to its sound and the sentences are joined with 1.5 seconds of silence into
// one 16 kHz WAV in a temporary folder, deleted afterwards. A sentence ends
// at its last sample above the threshold. src-tauri/examples/live_latency.rs
// feeds the file at the speed it was spoken and reports each phrase.
//
// A sentence that ends before the model has loaded is not counted; every
// one after that is. Each counted sentence must appear as exactly one phrase,
// in its own language, no more than 5.0 seconds after it ends.
//
//   npm run verify:live -- --label quiet      browsers and video meetings closed
//   npm run verify:live -- --label meeting    a Zoom test meeting open
//   npm run verify:live -- --label quiet --end-silence 0.5   try other endpoint numbers
//   npm run verify:live -- --fetch            download the selected model first
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { installedVoices, readWav } from "./eval-audio.mjs";

// Requirement 5: no more than this many seconds from the end of a sentence
// to its words.
export const LIMIT_SECONDS = 5;
export const SENTENCE_GAP_SECONDS = 1.5;
// A 16-bit sample louder than this is sound: about -60 dBFS.
export const SOUND_THRESHOLD = 33;
export const VOICES = { zh: "Tingting", en: "Samantha" };
const SHORT_SECONDS = [1, 20];
const LONG_SECONDS = [21, 30];
const RATE = 16_000;
const LABELS = ["quiet", "meeting"];
// What may be open on this Mac during a run: [name, executable names].
const WATCHED = [
  ["Zoom", ["zoom.us"]],
  ["Microsoft Teams", ["MSTeams", "Microsoft Teams"]],
  ["Google Chrome", ["Google Chrome"]],
  ["Safari", ["Safari"]],
  ["Microsoft Edge", ["Microsoft Edge"]],
  ["Arc", ["Arc"]],
  ["Firefox", ["firefox"]],
];
const TUNING = ["--end-silence", "--sound-db", "--min-sound", "--floor", "--pad"];

const han = /\p{Script=Han}/gu;
const latin = /[A-Za-z]/g;

// "zh" or "en" by the share of Han characters and Latin letters.
export function scriptOf(text) {
  const chinese = (text.match(han) ?? []).length;
  const letters = (text.match(latin) ?? []).length;
  if (chinese === 0 && letters === 0) return null;
  return chinese >= letters ? "zh" : "en";
}

export function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

const round = (value) => (value == null ? null : Number(value.toFixed(3)));

// First and last index of a sample above the threshold.
function soundSpan(samples) {
  let first = -1;
  let last = -1;
  for (let i = 0; i < samples.length; i++) {
    if (Math.abs(samples[i]) > SOUND_THRESHOLD) {
      if (first < 0) first = i;
      last = i;
    }
  }
  return first < 0 ? null : { first, last };
}

// One track: silence, then each clip trimmed to its sound with the gap
// after it. Returns the samples and where each sentence starts and ends.
export function layOut(clips, rate) {
  const gap = Math.round(SENTENCE_GAP_SECONDS * rate);
  const parts = [];
  const sentences = [];
  let at = gap;
  for (const clip of clips) {
    const span = soundSpan(clip.samples);
    if (!span) throw new Error(`${clip.id} has no sound`);
    const sound = clip.samples.subarray(span.first, span.last + 1);
    parts.push(sound);
    sentences.push({
      id: clip.id,
      lang: clip.lang,
      long: clip.long,
      start_seconds: at / rate,
      end_seconds: (at + sound.length) / rate,
    });
    at += sound.length + gap;
  }
  const samples = new Int16Array(at);
  let offset = gap;
  for (const part of parts) {
    samples.set(part, offset);
    offset += part.length + gap;
  }
  return { samples, sentences };
}

// Problems with the list of sentences: 12 Chinese, 12 English, and one long
// English sentence.
export function checkSentenceList(list) {
  const problems = [];
  const short = (lang) => list.filter((s) => s.lang === lang && !s.long).length;
  if (short("zh") !== 12) problems.push(`There must be 12 Chinese sentences, not ${short("zh")}.`);
  if (short("en") !== 12) problems.push(`There must be 12 English sentences, not ${short("en")}.`);
  const long = list.filter((s) => s.long);
  if (long.length !== 1 || long[0].lang !== "en") {
    problems.push("There must be exactly one long English sentence.");
  }
  if (new Set(list.map((s) => s.id)).size !== list.length) problems.push("Ids must be unique.");
  for (const s of list) {
    if (!VOICES[s.lang]) problems.push(`${s.id}: unknown language ${s.lang}.`);
    if (!s.text?.trim()) problems.push(`${s.id}: no text.`);
  }
  return problems;
}

// Short sentences are 1 to 20 seconds; the long one is about 25.
export function checkDurations(sentences) {
  return sentences.flatMap((s) => {
    const seconds = s.end_seconds - s.start_seconds;
    const [min, max] = s.long ? LONG_SECONDS : SHORT_SECONDS;
    return seconds >= min && seconds <= max
      ? []
      : [`${s.id} is ${seconds.toFixed(2)} s; it must be ${min} to ${max} s.`];
  });
}

// `say -v '?'` lists Tingting as "Tingting (Chinese (China mainland))".
export function hasVoice(installed, voice) {
  return installed.some((name) => name === voice || name.startsWith(`${voice} (`));
}

// The apps in WATCHED that appear in `ps -axo comm=` output.
export function runningApps(listing) {
  const running = new Set(listing.split("\n").map((line) => path.basename(line.trim())));
  return WATCHED.filter(([, names]) => names.some((n) => running.has(n))).map(([name]) => name);
}

// quiet: browsers and video meetings closed. meeting: Zoom open.
export function checkCondition(label, apps) {
  if (!LABELS.includes(label)) return [`--label must be ${LABELS.join(" or ")}.`];
  if (label === "quiet" && apps.length) {
    return [`A quiet run needs browsers and video meetings closed; open: ${apps.join(", ")}.`];
  }
  if (label === "meeting" && !apps.includes("Zoom")) {
    return ["A meeting run needs a Zoom test meeting open."];
  }
  return [];
}

// Matches phrases to sentences and checks each counted sentence.
export function scoreRun(sentences, output) {
  const loaded = output.model_loaded_seconds ?? null;
  const phrases = output.phrases ?? [];
  const problems = [];
  if (output.error) problems.push(`The run stopped: ${output.error}`);
  if (loaded == null) problems.push("The transcription model never loaded.");
  const overlaps = (p, s) =>
    p.start_seconds < s.end_seconds && p.voice_end_seconds > s.start_seconds;

  const rows = sentences.map((s) => {
    const mine = phrases.filter((p) => overlaps(p, s));
    const last = mine.at(-1);
    const latency = last ? last.handed_seconds - s.end_seconds : null;
    const text = mine.map((p) => p.text).join(" / ");
    const row = {
      id: s.id,
      lang: s.lang,
      long: Boolean(s.long),
      end_seconds: round(s.end_seconds),
      counted: loaded != null && s.end_seconds >= loaded,
      phrases: mine.length,
      latency_seconds: round(latency),
      transcribe_seconds: round(last?.transcribe_seconds),
      heard_as: last ? scriptOf(text) : null,
      text,
    };
    if (!row.counted) return row;
    const fail = (why) => problems.push(`${s.id}: ${why}`);
    if (mine.length === 0) fail("never appeared.");
    if (mine.length > 1) fail(`split into ${mine.length} phrases.`);
    const others = sentences.filter((o) => o !== s && mine.some((p) => overlaps(p, o)));
    if (others.length) fail(`merged with ${others.map((o) => o.id).join(", ")}.`);
    if (latency != null && latency > LIMIT_SECONDS) {
      fail(`appeared ${latency.toFixed(2)} s after it ended.`);
    }
    if (last && row.heard_as !== s.lang) fail(`came out as ${row.heard_as ?? "nothing"}.`);
    return row;
  });

  for (const p of phrases) {
    if (loaded != null && p.voice_end_seconds >= loaded && !sentences.some((s) => overlaps(p, s))) {
      problems.push(`A phrase where nothing was said: "${p.text}"`);
    }
  }
  const counted = rows.filter((row) => row.counted);
  if (loaded != null && counted.length === 0) {
    problems.push("No sentence ended after the model loaded.");
  }
  const latencies = counted.map((row) => row.latency_seconds).filter((v) => v != null);
  return {
    ok: problems.length === 0,
    counted: counted.length,
    max_latency_seconds: latencies.length ? Math.max(...latencies) : null,
    median_latency_seconds: round(median(latencies)),
    long_latency_seconds: rows.find((row) => row.long)?.latency_seconds ?? null,
    problems,
    sentences: rows,
  };
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: "utf8", maxBuffer: 1 << 26, ...options });
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    throw new Error(`${command} ${args.join(" ")} ${result.error ? "timed out" : "failed"}`);
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

const apps = () => runningApps(run("ps", ["-axo", "comm="]));

// `say` now and then hangs on a line it reads fine the next time.
function speak(voice, text, dir, id) {
  const textFile = path.join(dir, `${id}.txt`);
  const aiff = path.join(dir, `${id}.aiff`);
  const wav = path.join(dir, `${id}.wav`);
  writeFileSync(textFile, text);
  for (let tries = 1; ; tries++) {
    try {
      run("say", ["-v", voice, "-o", aiff, "-f", textFile], { timeout: 120_000 });
      break;
    } catch (err) {
      if (tries === 3) throw err;
      process.stderr.write(`${err.message}; trying again\n`);
    }
  }
  run("afconvert", ["-f", "WAVE", "-d", `LEI16@${RATE}`, "-c", "1", aiff, wav]);
  const { format, data } = readWav(readFileSync(wav));
  if (format.rate !== RATE || format.channels !== 1 || format.bits !== 16) {
    throw new Error(`${id}: afconvert wrote an unexpected format`);
  }
  const samples = new Int16Array(data.length / 2);
  for (let i = 0; i < samples.length; i++) samples[i] = data.readInt16LE(i * 2);
  return samples;
}

function writeWav(file, samples, rate) {
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + samples.length * 2, 4);
  header.write("WAVEfmt ", 8, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(1, 22);
  header.writeUInt32LE(rate, 24);
  header.writeUInt32LE(rate * 2, 28);
  header.writeUInt16LE(2, 32);
  header.writeUInt16LE(16, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(samples.length * 2, 40);
  writeFileSync(file, Buffer.concat([header, Buffer.from(samples.buffer)]));
}

const mb = (bytes) => (bytes == null ? null : Math.round(bytes / 2 ** 20));

function main(args) {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  const tauriDir = path.join(root, "src-tauri");
  const runner = path.join(tauriDir, "target", "release", "examples", "live_latency");
  const label = args[args.indexOf("--label") + 1];
  const tuning = [];
  for (const flag of TUNING) {
    const at = args.indexOf(flag);
    if (at >= 0) tuning.push(flag, args[at + 1]);
  }
  if (!args.includes("--label")) throw new Error(`--label must be ${LABELS.join(" or ")}.`);
  if (!LABELS.includes(label)) throw new Error(checkCondition(label, [])[0]);

  const list = JSON.parse(readFileSync(path.join(root, "scripts", "live-sentences.json"), "utf8"));
  const listProblems = checkSentenceList(list);
  if (listProblems.length) throw new Error(listProblems.join("\n"));
  const voices = installedVoices(run("say", ["-v", "?"]));
  for (const voice of Object.values(VOICES)) {
    if (!hasVoice(voices, voice)) {
      throw new Error(
        `The voice ${voice} is not on this Mac. Add it in System Settings > Accessibility > Spoken Content.`,
      );
    }
  }

  console.error("=== Building the live latency runner (release)");
  run("cargo", ["build", "--release", "--example", "live_latency"], {
    cwd: tauriDir,
    stdio: "inherit",
  });
  // Downloads the selected transcription model only if it is not on this Mac.
  run(runner, ["--fetch"], { stdio: "inherit" });

  const dir = mkdtempSync(path.join(os.tmpdir(), "anchovy-live-"));
  let output;
  let sentences;
  const appsBefore = apps();
  try {
    console.error(`=== Reading ${list.length} sentences with say`);
    const clips = list.map((s) => ({
      id: s.id,
      lang: s.lang,
      long: Boolean(s.long),
      samples: speak(VOICES[s.lang], s.text, dir, s.id),
    }));
    const track = layOut(clips, RATE);
    sentences = track.sentences;
    const durationProblems = checkDurations(sentences);
    if (durationProblems.length) throw new Error(durationProblems.join("\n"));
    const wav = path.join(dir, "sentences.wav");
    writeWav(wav, track.samples, RATE);
    const seconds = track.samples.length / RATE;
    console.error(`=== Feeding ${seconds.toFixed(0)} s of speech at the speed it was spoken`);
    output = JSON.parse(run(runner, [wav, ...tuning], { stdio: ["ignore", "pipe", "inherit"] }));
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
  const running = [...new Set([...appsBefore, ...apps()])];

  const result = scoreRun(sentences, output);
  const problems = [...checkCondition(label, running), ...result.problems];
  const report = {
    label,
    ok: problems.length === 0,
    date: new Date().toISOString(),
    machine: machine(),
    apps_running: running,
    model: output.model,
    params: output.params,
    model_loaded_seconds: round(output.model_loaded_seconds),
    counted: result.counted,
    max_latency_seconds: result.max_latency_seconds,
    median_latency_seconds: result.median_latency_seconds,
    long_latency_seconds: result.long_latency_seconds,
    peak_footprint_mb: mb(output.peak_footprint),
    footprint_before_mb: mb(output.footprint_before),
    available_memory_mb: mb(output.available_bytes_before),
    max_feed_lag_seconds: round(output.max_feed_lag_seconds),
    problems,
    sentences: result.sentences,
  };

  console.error("\nsentence  counted  latency  transcribe  text");
  for (const row of result.sentences) {
    const latency = row.latency_seconds == null ? "-" : `${row.latency_seconds.toFixed(2)} s`;
    const took = row.transcribe_seconds == null ? "-" : `${row.transcribe_seconds.toFixed(2)} s`;
    console.error(
      `${row.id.padEnd(9)} ${(row.counted ? "yes" : "no").padEnd(8)} ${latency.padStart(7)}  ${took.padStart(10)}  ${row.text}`,
    );
  }
  console.error(
    `\nmodel loaded at ${report.model_loaded_seconds} s; ${report.counted} counted; ` +
      `max ${report.max_latency_seconds} s, median ${report.median_latency_seconds} s, ` +
      `long sentence ${report.long_latency_seconds} s; peak ${report.peak_footprint_mb} MB; ` +
      `available before ${report.available_memory_mb} MB; open: ${running.join(", ") || "none"}`,
  );
  for (const problem of problems) console.error(`FAIL  ${problem}`);

  const reportsDir = path.join(root, "node_modules", ".tmp", "verify-live");
  mkdirSync(reportsDir, { recursive: true });
  const reportFile = path.join(reportsDir, `${label}-${report.date.replace(/[:.]/g, "-")}.json`);
  writeFileSync(reportFile, `${JSON.stringify(report, null, 2)}\n`);
  console.log(JSON.stringify(report, null, 2));
  console.error(`\nverify:live ${report.ok ? "passed" : "FAILED"} (${reportFile})`);
  return report.ok ? 0 : 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exit(main(process.argv.slice(2)));
  } catch (err) {
    console.error(`verify:live: ${err.message}`);
    process.exit(1);
  }
}
