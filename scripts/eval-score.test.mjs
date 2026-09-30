import { describe, expect, test } from "vitest";
import {
  checkJoins,
  compareCost,
  compareSampleError,
  compareWithBaseline,
  covered,
  matches,
  errorRate,
  languageOf,
  support,
  summaryCalls,
  SUPPORT_THRESHOLD,
  units,
} from "./eval-score.mjs";

describe("error rates", () => {
  test("Chinese is scored by characters and English by words", () => {
    expect(units("我们周五，发布。", "zh")).toEqual(["我", "们", "周", "五", "发", "布"]);
    expect(units("Ship it, on Friday!", "en")).toEqual(["ship", "it", "on", "friday"]);
  });

  test("edits count substitutions, insertions, and deletions", () => {
    expect(errorRate("我们下周一开会", "我们下周二开会", "zh")).toEqual({ edits: 1, length: 7 });
    expect(errorRate("Ship it on Friday.", "ship it friday", "en")).toEqual({
      edits: 1,
      length: 4,
    });
    expect(errorRate("a b", "a b c", "en")).toEqual({ edits: 1, length: 2 });
  });
});

describe("language", () => {
  test("follows the script most of the text uses", () => {
    expect(languageOf("我们讨论了 Anchovy 的发布。")).toBe("zh");
    expect(languageOf("We shipped 我们 on Friday.")).toBe("en");
    expect(languageOf("123 …")).toBeNull();
  });
});

describe("support", () => {
  const english =
    "Maria says she can fix both by Wednesday. We agreed to move the explanation above the Allow button.";
  const chinese = "王磊说周三之前能改完。我的建议是先不换模型，把分段改短一点，下周再测一次。";

  test("a paraphrase of what was said is supported", () => {
    expect(support("Maria fixes both issues by Wednesday", english, "en")).toBeGreaterThanOrEqual(
      SUPPORT_THRESHOLD,
    );
    expect(support("Move the explanation above the Allow button.", english, "en")).toBe(1);
    expect(support("王磊周三前改完", chinese, "zh")).toBeGreaterThanOrEqual(SUPPORT_THRESHOLD);
    expect(support("下周再测一次模型", chinese, "zh")).toBeGreaterThanOrEqual(SUPPORT_THRESHOLD);
  });

  test("an item the meeting never mentioned is not", () => {
    expect(support("Launch a marketing campaign in December", english, "en")).toBeLessThan(
      SUPPORT_THRESHOLD,
    );
    expect(support("李明负责准备市场推广方案", chinese, "zh")).toBeLessThan(SUPPORT_THRESHOLD);
  });

  test("checked items are matched to output items", () => {
    const items = ["Maria fixes the two issues by Wednesday.", "Tom orders the Mac today."];
    expect(covered(["Maria fixes both issues by Wednesday."], items, "en")).toBe(1);
    expect(covered(["Priya updates the design by Thursday."], items, "en")).toBe(0);
  });
});

describe("baseline", () => {
  const baseline = { chinese_cer: 0.01, english_wer: 0.05 };

  test("up to two points above the baseline passes", () => {
    expect(compareWithBaseline({ chinese_cer: 0.03, english_wer: 0.07 }, baseline)).toEqual([]);
    expect(compareWithBaseline({ chinese_cer: 0.0, english_wer: 0.0 }, baseline)).toEqual([]);
  });

  test("more than two points above fails and says which", () => {
    const failures = compareWithBaseline({ chinese_cer: 0.031, english_wer: 0.05 }, baseline);
    expect(failures).toEqual([
      "Chinese character error rate 3.1% is more than 2 points above the baseline 1.0%.",
    ]);
  });
});

describe("mixed Chinese and English", () => {
  test("Chinese is scored by characters and English by words in one text", () => {
    expect(units("我们用 Metal 跑，ship on Friday。", "mixed")).toEqual([
      "我",
      "们",
      "用",
      "metal",
      "跑",
      "ship",
      "on",
      "friday",
    ]);
  });

  test("support counts Chinese characters and English words", () => {
    const said = "我们决定先上 beta，下周一 release。";
    expect(support("下周一 release beta", said, "mixed")).toBe(1);
    expect(support("Marketing 下个月开始", said, "mixed")).toBeLessThan(SUPPORT_THRESHOLD);
  });
});

describe("support in a long meeting", () => {
  // Filler that shares no content words with the items below.
  const filler = (n) =>
    Array.from({ length: n }, (_, i) => `Topic ${i} covered numbers only.`).join(" ");

  test("an item said in one place is supported", () => {
    const transcript = `${filler(200)} Maria fixes the export bug by Wednesday. ${filler(200)}`;
    expect(support("Maria fixes the export bug by Wednesday", transcript, "en")).toBe(1);
  });

  test("words scattered across an hour do not support an item", () => {
    const transcript = `Maria spoke. ${filler(200)} The export works. ${filler(200)} Wednesday then.`;
    expect(support("Maria fixes the export by Wednesday", transcript, "en")).toBeLessThan(
      SUPPORT_THRESHOLD,
    );
  });
});

describe("window joins", () => {
  const reference = "one two three four five six seven eight nine ten eleven twelve";
  const segment = (start_seconds, text) => ({ start_seconds, text });
  // What the model wrote for two windows that share "six seven".
  const heard = ["one two three four five six seven", "six seven eight nine ten eleven twelve"];

  test("a clean join loses and repeats nothing", () => {
    const joins = checkJoins(
      reference,
      [
        segment(0, "one two three four five six seven"),
        segment(27, "eight nine ten eleven twelve"),
      ],
      heard,
      "en",
    );
    expect(joins).toEqual([{ start_seconds: 27, lost: 0, repeated: 0 }]);
  });

  test("words kept from both windows are repeated", () => {
    const [join] = checkJoins(
      reference,
      [
        segment(0, "one two three four five six seven"),
        segment(27, "six seven eight nine ten eleven twelve"),
      ],
      heard,
      "en",
    );
    expect(join).toEqual({ start_seconds: 27, lost: 0, repeated: 2 });
  });

  test("words cut from both windows are lost", () => {
    const [join] = checkJoins(
      reference,
      [segment(0, "one two three four five"), segment(27, "eight nine ten eleven twelve")],
      heard,
      "en",
    );
    expect(join).toEqual({ start_seconds: 27, lost: 2, repeated: 0 });
  });

  test("words the model never wrote are not lost at the join", () => {
    const [join] = checkJoins(
      reference,
      [segment(0, "one two three four"), segment(27, "nine ten eleven twelve")],
      ["one two three four", "nine ten eleven twelve"],
      "en",
    );
    expect(join).toEqual({ start_seconds: 27, lost: 0, repeated: 0 });
  });

  test("words the model made up are not repeated at the join", () => {
    const [join] = checkJoins(
      reference,
      [
        segment(0, "one two three four five six seven um well"),
        segment(27, "eight nine ten eleven twelve"),
      ],
      ["one two three four five six seven um well", "six seven eight nine ten eleven twelve"],
      "en",
    );
    expect(join).toEqual({ start_seconds: 27, lost: 0, repeated: 0 });
  });

  test("Chinese joins are checked by characters", () => {
    const [join] = checkJoins(
      "我们下周一开会讨论发布",
      [segment(0, "我们下周一"), segment(27, "周一开会讨论发布")],
      ["我们下周一", "周一开会讨论发布"],
      "zh",
    );
    expect(join).toEqual({ start_seconds: 27, lost: 0, repeated: 2 });
  });

  test("each join is checked on its own", () => {
    const joins = checkJoins(
      reference,
      [
        segment(0, "one two three four"),
        segment(27, "five six seven eight"),
        segment(54, "eight nine ten eleven twelve"),
      ],
      ["one two three four five", "four five six seven eight", "eight nine ten eleven twelve"],
      "en",
    );
    expect(joins).toEqual([
      { start_seconds: 27, lost: 0, repeated: 0 },
      { start_seconds: 54, lost: 0, repeated: 1 },
    ]);
  });
});

describe("summary chunks", () => {
  test("counts chunk and merge answers, not retries", () => {
    expect(
      summaryCalls([
        { kind: "chunk", attempt: 0 },
        { kind: "chunk", attempt: 0 },
        { kind: "chunk", attempt: 1 },
        { kind: "merge", attempt: 0 },
      ]),
    ).toEqual({ chunks: 2, merges: 1 });
    expect(summaryCalls([{ kind: "chunk", attempt: 0 }])).toEqual({ chunks: 1, merges: 0 });
  });
});

describe("hour cost", () => {
  const baseline = {
    seconds: { audio: 3600, transcribe: 400, summarize: 100 },
    memory: { peak_footprint: 2e9 },
  };
  const run = (transcribe, summarize, peak) => ({
    seconds: { audio: 3600, transcribe, summarize },
    memory: { peak_footprint: peak },
  });

  test("up to 20% worse than the baseline passes", () => {
    expect(compareCost("mixed-hour", run(480, 120, 2.4e9), baseline)).toEqual([]);
    expect(compareCost("mixed-hour", run(300, 50, 1e9), baseline)).toEqual([]);
  });

  test("more than 20% worse fails and says which", () => {
    expect(compareCost("mixed-hour", run(481, 100, 2.5e9), baseline)).toEqual([
      "mixed-hour: transcription took 481.0 s, more than 20% over the baseline 400.0 s.",
      "mixed-hour: peak memory 2500 MB is more than 20% over the baseline 2000 MB.",
    ]);
    expect(compareCost("mixed-hour", run(400, 121, 2e9), baseline)).toEqual([
      "mixed-hour: summary took 121.0 s, more than 20% over the baseline 100.0 s.",
    ]);
  });
});

describe("matches", () => {
  // Deletions plus insertions of the shortest script, from the longest
  // common subsequence.
  function shortest(a, b) {
    const lcs = Array.from({ length: a.length + 1 }, () => new Array(b.length + 1).fill(0));
    for (let i = 1; i <= a.length; i++) {
      for (let j = 1; j <= b.length; j++) {
        lcs[i][j] =
          a[i - 1] === b[j - 1] ? lcs[i - 1][j - 1] + 1 : Math.max(lcs[i - 1][j], lcs[i][j - 1]);
      }
    }
    return a.length + b.length - 2 * lcs[a.length][b.length];
  }

  test("pairs up the longest common subsequence in order", () => {
    let seed = 7;
    const next = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) % 4;
    for (let round = 0; round < 200; round++) {
      const a = Array.from({ length: next() * 3 }, next);
      const b = Array.from({ length: next() * 3 }, next);
      const pairs = matches(a, b);
      expect(a.length + b.length - 2 * pairs.length).toBe(shortest(a, b));
      for (const [i, j] of pairs) expect(a[i]).toBe(b[j]);
      pairs.slice(1).forEach(([i, j], n) => {
        expect(i).toBeGreaterThan(pairs[n][0]);
        expect(j).toBeGreaterThan(pairs[n][1]);
      });
    }
  });
});

describe("mixed error rate", () => {
  test("up to two points above the sample's baseline passes", () => {
    expect(compareSampleError("mixed-hour", 0.066, { error_rate: 0.046 })).toEqual([]);
  });

  test("more than two points above fails", () => {
    expect(compareSampleError("mixed-hour", 0.067, { error_rate: 0.046 })).toEqual([
      "mixed-hour: error rate 6.7% is more than 2 points above the baseline 4.6%.",
    ]);
  });
});
