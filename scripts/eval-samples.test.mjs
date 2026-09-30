import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, test } from "vitest";
import { parseScript } from "./eval-audio.mjs";
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
