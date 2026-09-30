// Audio for `npm run eval`. macOS's built-in voices read each sample's
// script, and afconvert turns it into the app's recording format: 48 kHz
// 16-bit mono WAV. A sample with several speakers is read turn by turn and
// the clips are joined with a short silence. A sample may add background
// noise, made here from a fixed seed at a stated level below the speech. The
// audio is never committed.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

// Silence between two turns, like a speaker change.
export const TURN_GAP_SECONDS = 0.6;
const RECORDING_FORMAT = ["-f", "WAVE", "-d", "LEI16@48000", "-c", "1"];

// `Voice: text`, one turn per line. Blank lines are skipped. A voice name is
// what `say -v` takes, such as `Flo (Chinese (China mainland))`.
export function parseScript(text) {
  const turns = [];
  text.split("\n").forEach((line, index) => {
    if (!line.trim()) return;
    const match = line.match(/^([A-Za-z]+(?: \([^:]*\))?): (.+)$/);
    if (!match) throw new Error(`script line ${index + 1} does not start with "Voice: "`);
    turns.push({ voice: match[1], text: match[2].trim() });
  });
  return turns;
}

// The format and the sample bytes of a PCM WAV file.
export function readWav(buffer) {
  if (buffer.toString("ascii", 0, 4) !== "RIFF" || buffer.toString("ascii", 8, 12) !== "WAVE") {
    throw new Error("not a WAV file");
  }
  let format = null;
  let data = null;
  for (let at = 12; at + 8 <= buffer.length;) {
    const id = buffer.toString("ascii", at, at + 4);
    const size = buffer.readUInt32LE(at + 4);
    const body = buffer.subarray(at + 8, at + 8 + size);
    if (id === "fmt ") {
      format = {
        encoding: body.readUInt16LE(0),
        channels: body.readUInt16LE(2),
        rate: body.readUInt32LE(4),
        bits: body.readUInt16LE(14),
      };
    } else if (id === "data") {
      data = body;
    }
    at += 8 + size + (size % 2);
  }
  if (!format || !data) throw new Error("WAV file without fmt or data");
  return { format, data };
}

// Voice names in the output of `say -v '?'`.
export function installedVoices(listing) {
  return listing
    .split("\n")
    .map((line) => line.match(/^(.+?)\s+[a-z]{2,3}_[A-Z0-9]{2,4}\s+#/)?.[1])
    .filter(Boolean);
}

// 16-bit PCM WAV bytes for `data` in `format`.
function writeWav(format, data) {
  const frameBytes = format.channels * (format.bits / 8);
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + data.length, 4);
  header.write("WAVEfmt ", 8, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(format.encoding, 20);
  header.writeUInt16LE(format.channels, 22);
  header.writeUInt32LE(format.rate, 24);
  header.writeUInt32LE(format.rate * frameBytes, 28);
  header.writeUInt16LE(frameBytes, 32);
  header.writeUInt16LE(format.bits, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(data.length, 40);
  return Buffer.concat([header, data]);
}

// One WAV file of `clips` in order, `gapSamples` of silence between them.
export function concatWav(clips, gapSamples) {
  const parsed = clips.map(readWav);
  const format = parsed[0].format;
  for (const clip of parsed) {
    if (JSON.stringify(clip.format) !== JSON.stringify(format)) {
      throw new Error("clips differ in format");
    }
  }
  const frameBytes = format.channels * (format.bits / 8);
  const gap = Buffer.alloc(gapSamples * frameBytes);
  const parts = parsed.flatMap((clip, i) => (i === 0 ? [clip.data] : [gap, clip.data]));
  return writeWav(format, Buffer.concat(parts));
}

// Background noise: white (hiss), pink (a room or a fan), brown (a low rumble,
// like air conditioning or traffic).
export const NOISE_TYPES = ["white", "pink", "brown"];

// Mulberry32: the same seed gives the same numbers on every Mac.
function seededRandom(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 2 ** 32;
  };
}

// `count` samples of noise from `seed`, with no DC offset and a root mean
// square of 1.
export function noise(type, count, seed) {
  if (!NOISE_TYPES.includes(type)) throw new Error(`unknown noise type ${type}`);
  const random = seededRandom(seed);
  const out = new Float64Array(count);
  // Paul Kellet's pink filter; brown is white noise through a leaky integrator.
  let [b0, b1, b2, b3, b4, b5, b6] = [0, 0, 0, 0, 0, 0, 0];
  let brown = 0;
  for (let i = 0; i < count; i++) {
    const white = random() * 2 - 1;
    if (type === "white") {
      out[i] = white;
    } else if (type === "pink") {
      b0 = 0.99886 * b0 + white * 0.0555179;
      b1 = 0.99332 * b1 + white * 0.0750759;
      b2 = 0.969 * b2 + white * 0.153852;
      b3 = 0.8665 * b3 + white * 0.3104856;
      b4 = 0.55 * b4 + white * 0.5329522;
      b5 = -0.7616 * b5 - white * 0.016898;
      out[i] = b0 + b1 + b2 + b3 + b4 + b5 + b6 + white * 0.5362;
      b6 = white * 0.115926;
    } else {
      brown = 0.998 * brown + white;
      out[i] = brown;
    }
  }
  const mean = out.reduce((sum, v) => sum + v, 0) / count;
  const rms = Math.sqrt(out.reduce((sum, v) => sum + (v - mean) ** 2, 0) / count);
  return out.map((v) => (v - mean) / rms);
}

// `wav` with noise `snr_db` decibels below the speech's root mean square over
// the whole clip, turn gaps included. Samples past full scale are clipped.
export function addNoise(wav, { type, snr_db, seed }) {
  const { format, data } = readWav(wav);
  const count = data.length / 2;
  let power = 0;
  for (let i = 0; i < count; i++) power += data.readInt16LE(i * 2) ** 2;
  const level = Math.sqrt(power / count) * 10 ** (-snr_db / 20);
  const added = noise(type, count, seed);
  const out = Buffer.alloc(data.length);
  for (let i = 0; i < count; i++) {
    const value = Math.round(data.readInt16LE(i * 2) + added[i] * level);
    out.writeInt16LE(Math.max(-32768, Math.min(32767, value)), i * 2);
  }
  return writeWav(format, out);
}

function run(command, args, timeout) {
  const result = spawnSync(command, args, { encoding: "utf8", timeout });
  if (result.status !== 0) {
    process.stderr.write(result.stderr ?? "");
    throw new Error(`${command} ${args.join(" ")} ${result.error ? "timed out" : "failed"}`);
  }
}

// `say` now and then hangs on a turn it reads fine the next time.
const SAY_TIMEOUT_MS = 120_000;
const SAY_TRIES = 3;

function speak(voice, textFile, wav) {
  const aiff = wav.replace(/\.wav$/, ".aiff");
  for (let tries = 1; ; tries++) {
    try {
      run("say", ["-v", voice, "-o", aiff, "-f", textFile], SAY_TIMEOUT_MS);
      break;
    } catch (err) {
      if (tries === SAY_TRIES) throw err;
      process.stderr.write(`${err.message}; trying again\n`);
    }
  }
  run("afconvert", [...RECORDING_FORMAT, aiff, wav]);
  rmSync(aiff);
}

let voices = null;

// `say` reads with a default voice, without an error, when the one named is
// not installed.
function checkVoices(names) {
  voices ??= new Set(installedVoices(spawnSync("say", ["-v", "?"], { encoding: "utf8" }).stdout));
  for (const name of names) {
    if (!voices.has(name)) {
      throw new Error(
        `The voice ${name} is not on this Mac. Add it in System Settings > Accessibility > Spoken Content.`,
      );
    }
  }
}

// Makes `<dir>/<id>.wav` for a sample: `transcript.txt` read by `voice`, or
// `script.txt` read turn by turn, then its noise if it has some.
export function makeClip(sample, sampleDir, dir) {
  mkdirSync(dir, { recursive: true });
  const wav = path.join(dir, `${sample.id}.wav`);
  checkVoices(sample.turns ? sample.turns.map((turn) => turn.voice) : [sample.voice]);
  if (!sample.turns) {
    speak(sample.voice, path.join(sampleDir, "transcript.txt"), wav);
  } else {
    joinTurns(sample, wav, dir);
  }
  if (sample.noise) writeFileSync(wav, addNoise(readFileSync(wav), sample.noise));
  return wav;
}

function joinTurns(sample, wav, dir) {
  const turnsDir = path.join(dir, sample.id);
  rmSync(turnsDir, { recursive: true, force: true });
  mkdirSync(turnsDir);
  const clips = sample.turns.map((turn, i) => {
    const name = String(i).padStart(4, "0");
    const textFile = path.join(turnsDir, `${name}.txt`);
    writeFileSync(textFile, turn.text);
    speak(turn.voice, textFile, path.join(turnsDir, `${name}.wav`));
    return readFileSync(path.join(turnsDir, `${name}.wav`));
  });
  const rate = readWav(clips[0]).format.rate;
  writeFileSync(wav, concatWav(clips, Math.round(TURN_GAP_SECONDS * rate)));
  rmSync(turnsDir, { recursive: true });
}
