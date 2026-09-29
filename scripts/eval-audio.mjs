// Audio for `npm run eval`. macOS's built-in voices read each sample's
// script, and afconvert turns it into the app's recording format: 48 kHz
// 16-bit mono WAV. A sample with several speakers is read turn by turn and
// the clips are joined with a short silence. The audio is never committed.
import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

// Silence between two turns, like a speaker change.
export const TURN_GAP_SECONDS = 0.6;
const RECORDING_FORMAT = ["-f", "WAVE", "-d", "LEI16@48000", "-c", "1"];

// `Voice: text`, one turn per line. Blank lines are skipped.
export function parseScript(text) {
  const turns = [];
  text.split("\n").forEach((line, index) => {
    if (!line.trim()) return;
    const match = line.match(/^([A-Za-z]+): (.+)$/);
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
  const dataBytes = parts.reduce((sum, part) => sum + part.length, 0);
  const header = Buffer.alloc(44);
  header.write("RIFF", 0, "ascii");
  header.writeUInt32LE(36 + dataBytes, 4);
  header.write("WAVEfmt ", 8, "ascii");
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(format.encoding, 20);
  header.writeUInt16LE(format.channels, 22);
  header.writeUInt32LE(format.rate, 24);
  header.writeUInt32LE(format.rate * frameBytes, 28);
  header.writeUInt16LE(frameBytes, 32);
  header.writeUInt16LE(format.bits, 34);
  header.write("data", 36, "ascii");
  header.writeUInt32LE(dataBytes, 40);
  return Buffer.concat([header, ...parts]);
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

// Makes `<dir>/<id>.wav` for a sample: `transcript.txt` read by `voice`, or
// `script.txt` read turn by turn.
export function makeClip(sample, sampleDir, dir) {
  mkdirSync(dir, { recursive: true });
  const wav = path.join(dir, `${sample.id}.wav`);
  if (!sample.turns) {
    speak(sample.voice, path.join(sampleDir, "transcript.txt"), wav);
    return wav;
  }
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
  return wav;
}
