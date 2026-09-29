import { describe, expect, test } from "vitest";
import { concatWav, parseScript, readWav } from "./eval-audio.mjs";

describe("script", () => {
  test("each line is one turn read by the voice named before the colon", () => {
    const script = "Tingting: 好，我们开始。\n\nSamantha: Thanks, let's start: first item.\n";
    expect(parseScript(script)).toEqual([
      { voice: "Tingting", text: "好，我们开始。" },
      { voice: "Samantha", text: "Thanks, let's start: first item." },
    ]);
  });

  test("a line without a voice is an error", () => {
    expect(() => parseScript("Tingting: 好。\n我们开始。")).toThrow("line 2");
  });
});

// A WAV file as afconvert writes it: a FLLR chunk may come before the data.
function wav(samples, { rate = 48000, filler = false } = {}) {
  const chunk = (id, body) => {
    const head = Buffer.alloc(8);
    head.write(id, 0, "ascii");
    head.writeUInt32LE(body.length, 4);
    return Buffer.concat([head, body]);
  };
  const fmt = Buffer.alloc(16);
  fmt.writeUInt16LE(1, 0);
  fmt.writeUInt16LE(1, 2);
  fmt.writeUInt32LE(rate, 4);
  fmt.writeUInt32LE(rate * 2, 8);
  fmt.writeUInt16LE(2, 12);
  fmt.writeUInt16LE(16, 14);
  const data = Buffer.alloc(samples.length * 2);
  samples.forEach((s, i) => data.writeInt16LE(s, i * 2));
  const body = Buffer.concat([
    Buffer.from("WAVE"),
    chunk("fmt ", fmt),
    ...(filler ? [chunk("FLLR", Buffer.alloc(12))] : []),
    chunk("data", data),
  ]);
  const riff = Buffer.alloc(8);
  riff.write("RIFF", 0, "ascii");
  riff.writeUInt32LE(body.length, 4);
  return Buffer.concat([riff, body]);
}

const samplesOf = (buffer) => {
  const { data } = readWav(buffer);
  return Array.from({ length: data.length / 2 }, (_, i) => data.readInt16LE(i * 2));
};

describe("wav", () => {
  test("reads the format and samples, skipping other chunks", () => {
    const parsed = readWav(wav([1, -2, 3], { filler: true }));
    expect(parsed.format).toMatchObject({ channels: 1, rate: 48000, bits: 16 });
    expect(samplesOf(wav([1, -2, 3], { filler: true }))).toEqual([1, -2, 3]);
  });

  test("joins clips with silence between them", () => {
    const joined = concatWav([wav([1, 2]), wav([3], { filler: true })], 2);
    expect(readWav(joined).format).toMatchObject({ channels: 1, rate: 48000, bits: 16 });
    expect(samplesOf(joined)).toEqual([1, 2, 0, 0, 3]);
    expect(joined.readUInt32LE(4)).toBe(joined.length - 8);
  });

  test("clips in different formats are not joined", () => {
    expect(() => concatWav([wav([1]), wav([2], { rate: 16000 })], 0)).toThrow("format");
  });
});
