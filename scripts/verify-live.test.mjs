import { readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  checkCondition,
  checkDurations,
  checkSentenceList,
  layOut,
  LIMIT_SECONDS,
  hasVoice,
  median,
  runningApps,
  scoreRun,
  scriptOf,
  SENTENCE_GAP_SECONDS,
} from "./verify-live.mjs";

const zh = { id: "zh-1", lang: "zh", long: false, start_seconds: 1.5, end_seconds: 4 };
const en = { id: "en-1", lang: "en", long: false, start_seconds: 5.5, end_seconds: 10 };
const later = { id: "en-2", lang: "en", long: true, start_seconds: 11.5, end_seconds: 37 };

function phrase(start, voiceEnd, handed, text) {
  return {
    start_seconds: start,
    voice_end_seconds: voiceEnd,
    cut_seconds: voiceEnd + 0.6,
    handed_seconds: handed,
    transcribe_seconds: 1,
    text,
  };
}

function run(phrases, loaded = 0) {
  return { model_loaded_seconds: loaded, phrases };
}

const good = [
  phrase(1.3, 4, 6, "我们下周三再开一次会。"),
  phrase(5.3, 10, 12, "Let's meet again next Wednesday."),
  phrase(11.3, 37, 40, "This is the long one that keeps going."),
];

describe("scoreRun", () => {
  it("passes when every sentence appears once, in time, in its own language", () => {
    const result = scoreRun([zh, en, later], run(good));
    expect(result.problems).toEqual([]);
    expect(result.ok).toBe(true);
    expect(result.counted).toBe(3);
    expect(result.max_latency_seconds).toBe(3);
    expect(result.median_latency_seconds).toBe(2);
    expect(result.long_latency_seconds).toBe(3);
  });

  it("does not count a sentence finished while the model was loading", () => {
    const slow = [phrase(1.3, 4, 11, "我们下周三再开一次会。"), ...good.slice(1)];
    const result = scoreRun([zh, en, later], run(slow, 4.5));
    expect(result.ok).toBe(true);
    expect(result.counted).toBe(2);
    expect(result.sentences[0]).toMatchObject({ counted: false, latency_seconds: 7 });
    expect(result.max_latency_seconds).toBe(3);
  });

  it("counts a sentence that ends after the model has loaded, however close", () => {
    const slow = [phrase(1.3, 4, 9.5, "我们下周三再开一次会。"), ...good.slice(1)];
    const result = scoreRun([zh, en, later], run(slow, 3.99));
    expect(result.ok).toBe(false);
    expect(result.counted).toBe(3);
  });

  it("allows 5.0 seconds and fails 5.01", () => {
    expect(LIMIT_SECONDS).toBe(5);
    const at = (handed) => [good[0], phrase(5.3, 10, handed, "Let's meet."), good[2]];
    expect(scoreRun([zh, en, later], run(at(15))).ok).toBe(true);
    const late = scoreRun([zh, en, later], run(at(15.01)));
    expect(late.ok).toBe(false);
    expect(late.problems.join("\n")).toMatch(/en-1.*5\.01 s/);
  });

  it("fails a sentence split in two", () => {
    const split = [
      good[0],
      phrase(5.3, 7, 8, "Let's meet"),
      phrase(7.6, 10, 11, "again next Wednesday."),
      good[2],
    ];
    const result = scoreRun([zh, en, later], run(split));
    expect(result.ok).toBe(false);
    expect(result.problems.join("\n")).toMatch(/en-1.*split into 2/);
  });

  it("fails two sentences merged into one phrase", () => {
    const merged = [phrase(1.3, 10, 12, "我们下周三再开一次会。Let's meet again."), good[2]];
    const result = scoreRun([zh, en, later], run(merged));
    expect(result.ok).toBe(false);
    expect(result.problems.join("\n")).toMatch(/zh-1.*merged/);
    expect(result.problems.join("\n")).toMatch(/en-1.*merged/);
  });

  it("fails a Chinese sentence that comes out in English", () => {
    const translated = [phrase(1.3, 4, 6, "Let's meet again next Wednesday."), ...good.slice(1)];
    const result = scoreRun([zh, en, later], run(translated));
    expect(result.ok).toBe(false);
    expect(result.problems.join("\n")).toMatch(/zh-1.*came out as en/);
  });

  it("fails a sentence that never appears, and never counts it as in time", () => {
    const result = scoreRun([zh, en, later], run([good[0], good[2]]));
    expect(result.ok).toBe(false);
    expect(result.problems.join("\n")).toMatch(/en-1.*never appeared/);
    expect(result.sentences[1].latency_seconds).toBeNull();
  });

  it("fails a phrase where nothing was said", () => {
    const extra = [...good, phrase(40, 41, 42, "Thank you.")];
    const result = scoreRun([zh, en, later], run(extra));
    expect(result.ok).toBe(false);
    expect(result.problems.join("\n")).toMatch(/nothing was said.*Thank you/);
  });

  it("fails a run where no sentence could be counted", () => {
    const result = scoreRun([zh], run([good[0]], 100));
    expect(result.ok).toBe(false);
  });
});

describe("scriptOf", () => {
  it("compares Han characters with Latin letters", () => {
    expect(scriptOf("我们下周三再开一次会。")).toBe("zh");
    expect(scriptOf("Let's meet again next Wednesday.")).toBe("en");
    expect(scriptOf("我们用 Zoom 开会，下周三下午三点。")).toBe("zh");
    expect(scriptOf("")).toBeNull();
    expect(scriptOf("123 ...")).toBeNull();
  });
});

describe("layOut", () => {
  it("trims each clip to its sound and puts exactly the gap between them", () => {
    const rate = 1000;
    const clip = (lead, loud, tail) =>
      Int16Array.from([...Array(lead).fill(0), ...Array(loud).fill(3000), ...Array(tail).fill(5)]);
    const { samples, sentences } = layOut(
      [
        { id: "a", lang: "zh", long: false, samples: clip(100, 2000, 300) },
        { id: "b", lang: "en", long: false, samples: clip(50, 1000, 10) },
      ],
      rate,
    );
    const gap = SENTENCE_GAP_SECONDS * rate;
    expect(sentences).toEqual([
      { id: "a", lang: "zh", long: false, start_seconds: 1.5, end_seconds: 3.5 },
      { id: "b", lang: "en", long: false, start_seconds: 5, end_seconds: 6 },
    ]);
    expect(samples.length).toBe(gap + 2000 + gap + 1000 + gap);
    expect(samples[gap - 1]).toBe(0);
    expect(samples[gap]).toBe(3000);
  });
});

describe("the sentences", () => {
  const list = JSON.parse(
    readFileSync(path.join(import.meta.dirname, "live-sentences.json"), "utf8"),
  );

  it("are 12 Chinese, 12 English, and one long English sentence", () => {
    expect(checkSentenceList(list)).toEqual([]);
    expect(list.filter((s) => s.lang === "zh" && !s.long)).toHaveLength(12);
    expect(list.filter((s) => s.lang === "en" && !s.long)).toHaveLength(12);
    expect(list.filter((s) => s.long)).toEqual([expect.objectContaining({ lang: "en" })]);
  });

  it("are written in the language they are labeled with", () => {
    for (const sentence of list) expect(scriptOf(sentence.text), sentence.id).toBe(sentence.lang);
  });

  it("are checked for count and the long one", () => {
    expect(checkSentenceList(list.filter((s) => !s.long))).toContain(
      "There must be exactly one long English sentence.",
    );
    expect(checkSentenceList(list.slice(1))).not.toEqual([]);
  });
});

describe("checkDurations", () => {
  it("keeps short sentences between 1 and 20 seconds and the long one near 25", () => {
    const s = (id, seconds, long = false) => ({
      id,
      long,
      start_seconds: 0,
      end_seconds: seconds,
    });
    expect(checkDurations([s("a", 1), s("b", 20), s("c", 25, true)])).toEqual([]);
    expect(checkDurations([s("a", 0.9)])).toHaveLength(1);
    expect(checkDurations([s("a", 20.5)])).toHaveLength(1);
    expect(checkDurations([s("c", 19, true)])).toHaveLength(1);
  });
});

describe("checkCondition", () => {
  it("needs browsers and meetings closed for quiet, and Zoom open for meeting", () => {
    expect(checkCondition("quiet", [])).toEqual([]);
    expect(checkCondition("quiet", ["Google Chrome"])).toHaveLength(1);
    expect(checkCondition("meeting", ["Zoom"])).toEqual([]);
    expect(checkCondition("meeting", ["Safari"])).toHaveLength(1);
    expect(checkCondition("loud", [])).toHaveLength(1);
  });
});

describe("median", () => {
  it("is the middle value, or the mean of the two middle values", () => {
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 2, 3])).toBe(2.5);
    expect(median([])).toBeNull();
  });
});

describe("runningApps", () => {
  it("finds the watched apps by their executable name", () => {
    const listing = [
      "/Applications/zoom.us.app/Contents/MacOS/zoom.us",
      "/Applications/Safari.app/Contents/MacOS/Safari",
      "/System/Library/PrivateFrameworks/SafariSafeBrowsing.framework/com.apple.Safari.SafeBrowsing.Service",
      "/usr/sbin/cfprefsd",
    ].join("\n");
    expect(runningApps(listing)).toEqual(["Zoom", "Safari"]);
    expect(runningApps("/usr/sbin/cfprefsd\n")).toEqual([]);
  });
});

describe("hasVoice", () => {
  it("accepts the voice by its short name or its full listed name", () => {
    const installed = ["Tingting (Chinese (China mainland))", "Samantha (English (US))", "Flo"];
    expect(hasVoice(installed, "Tingting")).toBe(true);
    expect(hasVoice(installed, "Samantha")).toBe(true);
    expect(hasVoice(installed, "Flo")).toBe(true);
    expect(hasVoice(installed, "Ting")).toBe(false);
    expect(hasVoice(installed, "Meijia")).toBe(false);
  });
});
