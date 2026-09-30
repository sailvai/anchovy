import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, test } from "vitest";
import { NOISE_TYPES, parseScript } from "./eval-audio.mjs";
import { support, SUPPORT_THRESHOLD } from "./eval-score.mjs";

const samplesDir = path.resolve(import.meta.dirname, "..", "evals", "samples");

function load(id) {
  const dir = path.join(samplesDir, id);
  const sample = JSON.parse(readFileSync(path.join(dir, "sample.json"), "utf8"));
  const transcript = existsSync(path.join(dir, "script.txt"))
    ? parseScript(readFileSync(path.join(dir, "script.txt"), "utf8"))
        .map((turn) => turn.text)
        .join("\n")
    : readFileSync(path.join(dir, "transcript.txt"), "utf8");
  return { ...sample, transcript };
}

describe.each(readdirSync(samplesDir).sort())("eval sample %s", (id) => {
  const sample = load(id);
  const dir = path.join(samplesDir, id);
  const han = /\p{Script=Han}/u;
  const latin = /\p{Script=Latin}/u;

  test("says which language is spoken", () => {
    expect(["zh", "en", "mixed"]).toContain(sample.language);
  });

  test("every checked decision and action item was said in one place", () => {
    for (const item of [...sample.decisions, ...sample.action_items]) {
      expect({ item, support: support(item, sample.transcript, sample.language) }).toEqual({
        item,
        support: expect.toSatisfy((s) => s >= SUPPORT_THRESHOLD),
      });
    }
  });

  test("a sample with a script has several speakers", () => {
    if (!existsSync(path.join(dir, "script.txt"))) return;
    const turns = parseScript(readFileSync(path.join(dir, "script.txt"), "utf8"));
    expect(new Set(turns.map((turn) => turn.voice)).size).toBeGreaterThanOrEqual(2);
  });

  // The Eloquence Chinese voices misread Latin letters and English words
  // ("transcript" is heard as "ice cube", "八个G" as "八个刻"), so the
  // transcript would not be what was said. Tingting reads English well.
  test("a Chinese Eloquence voice reads no Latin letters", () => {
    if (!existsSync(path.join(dir, "script.txt"))) return;
    const misread = parseScript(readFileSync(path.join(dir, "script.txt"), "utf8")).filter(
      (turn) => turn.voice.endsWith("(Chinese (China mainland))") && latin.test(turn.text),
    );
    expect(misread).toEqual([]);
  });

  test("noise, when there is some, has a known type, a level, and a seed", () => {
    if (!sample.noise) return;
    expect(sample.noise).toEqual({
      type: expect.toSatisfy((type) => NOISE_TYPES.includes(type)),
      snr_db: expect.toSatisfy((db) => typeof db === "number" && db > 0 && db <= 40),
      seed: expect.toSatisfy(Number.isInteger),
    });
  });

  test("a mixed sample has both Chinese and English", () => {
    if (sample.language !== "mixed") return;
    expect(han.test(sample.transcript) && latin.test(sample.transcript)).toBe(true);
  });
});

describe("the hour-long sample", () => {
  const sample = load("mixed-hour");
  const turns = parseScript(
    readFileSync(path.join(samplesDir, "mixed-hour", "script.txt"), "utf8"),
  );
  const han = /\p{Script=Han}/u;
  const latin = /\p{Script=Latin}/u;

  test("has Chinese, English, and mixed turns", () => {
    const kinds = new Set(
      turns.map((turn) =>
        han.test(turn.text) && latin.test(turn.text) ? "mixed" : han.test(turn.text) ? "zh" : "en",
      ),
    );
    expect([...kinds].sort()).toEqual(["en", "mixed", "zh"]);
  });

  test("its decisions and action items come from all three", () => {
    const items = [...sample.decisions, ...sample.action_items];
    expect(items.some((item) => han.test(item) && !latin.test(item))).toBe(true);
    expect(items.some((item) => latin.test(item) && !han.test(item))).toBe(true);
    expect(items.some((item) => han.test(item) && latin.test(item))).toBe(true);
  });
});

// Plan section 10: at least 10 Chinese and 10 English samples, with several
// speakers, noise, and mixed Chinese and English.
describe("the sample set", () => {
  const samples = readdirSync(samplesDir)
    .sort()
    .map((id) => ({ id, ...load(id) }));
  const count = (language) => samples.filter((s) => s.language === language).length;

  test("has at least 10 Chinese, 10 English, and 4 mixed samples", () => {
    expect(count("zh")).toBeGreaterThanOrEqual(10);
    expect(count("en")).toBeGreaterThanOrEqual(10);
    expect(count("mixed")).toBeGreaterThanOrEqual(4);
  });

  test("has noise in some Chinese, English, and mixed samples", () => {
    for (const language of ["zh", "en", "mixed"]) {
      expect({ language, noisy: samples.some((s) => s.language === language && s.noise) }).toEqual({
        language,
        noisy: true,
      });
    }
  });
});
