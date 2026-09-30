import { describe, expect, test } from "vitest";
import {
  addNoise,
  concatWav,
  installedVoices,
  noise,
  NOISE_TYPES,
  parseScript,
  readWav,
} from "./eval-audio.mjs";

describe("script", () => {
  test("each line is one turn read by the voice named before the colon", () => {
    const script = "Tingting: 好，我们开始。\n\nSamantha: Thanks, let's start: first item.\n";
    expect(parseScript(script)).toEqual([
      { voice: "Tingting", text: "好，我们开始。" },
      { voice: "Samantha", text: "Thanks, let's start: first item." },
    ]);
  });

  test("a voice name may have a language in brackets, as macOS names them", () => {
    const script =
      "Flo (Chinese (China mainland)): 好，我们开始。\nEddy (English (UK)): Item one: budget.\n";
    expect(parseScript(script)).toEqual([
      { voice: "Flo (Chinese (China mainland))", text: "好，我们开始。" },
      { voice: "Eddy (English (UK))", text: "Item one: budget." },
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

describe("voices", () => {
  // `say -v '?'` as macOS prints it.
  const listing = [
    "Daniel              en_GB    # Hello! My name is Daniel.",
    "Flo (Chinese (China mainland)) zh_CN    # 你好！我叫Flo。",
    "Eddy (English (US)) en_US    # Hello! My name is Eddy.",
    "Tingting            zh_CN    # 你好！我叫婷婷。",
    "",
  ].join("\n");

  test("lists the installed voices by the name say takes", () => {
    expect(installedVoices(listing)).toEqual([
      "Daniel",
      "Flo (Chinese (China mainland))",
      "Eddy (English (US))",
      "Tingting",
    ]);
  });
});

const rms = (values) => Math.sqrt(values.reduce((sum, v) => sum + v * v, 0) / values.length);

describe("noise", () => {
  test.each(NOISE_TYPES)(
    "%s noise is the same for the same seed and scaled to unit level",
    (type) => {
      const a = noise(type, 48000, 7);
      expect(Array.from(a)).toEqual(Array.from(noise(type, 48000, 7)));
      expect(Array.from(a)).not.toEqual(Array.from(noise(type, 48000, 8)));
      expect(rms(Array.from(a))).toBeCloseTo(1, 6);
    },
  );

  test("pink and brown noise are darker than white noise", () => {
    // Share of the level left after a one-sample difference, a rough high-pass.
    const brightness = (type) => {
      const n = Array.from(noise(type, 48000, 3));
      return rms(n.slice(1).map((v, i) => v - n[i])) / rms(n);
    };
    expect(brightness("pink")).toBeLessThan(brightness("white") / 2);
    expect(brightness("brown")).toBeLessThan(brightness("pink"));
  });

  test("an unknown noise type is an error", () => {
    expect(() => noise("rain", 10, 1)).toThrow("rain");
  });

  test("is added at the stated level below the speech", () => {
    const speech = Array.from({ length: 48000 }, (_, i) =>
      Math.round(3000 * Math.sin((2 * Math.PI * 220 * i) / 48000)),
    );
    const mixed = samplesOf(addNoise(wav(speech), { type: "pink", snr_db: 15, seed: 5 }));
    const added = mixed.map((v, i) => v - speech[i]);
    const snr = 20 * Math.log10(rms(speech) / rms(added));
    expect(snr).toBeGreaterThan(14.9);
    expect(snr).toBeLessThan(15.1);
    expect(addNoise(wav(speech), { type: "pink", snr_db: 15, seed: 5 })).toEqual(
      addNoise(wav(speech), { type: "pink", snr_db: 15, seed: 5 }),
    );
  });

  test("keeps the format and clips instead of wrapping around", () => {
    // Near full scale: noise peaks would wrap past 32767 into negative values.
    const loud = new Array(4800).fill(32000);
    const out = addNoise(wav(loud, { filler: true }), { type: "white", snr_db: 20, seed: 1 });
    expect(readWav(out).format).toMatchObject({ channels: 1, rate: 48000, bits: 16 });
    const values = samplesOf(out);
    expect(values).toHaveLength(4800);
    expect(Math.max(...values)).toBe(32767);
    expect(Math.min(...values)).toBeGreaterThan(0);
  });
});
