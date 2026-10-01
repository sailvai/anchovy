import { describe, expect, test } from "vitest";
import {
  checkJoins,
  compareCost,
  compareSampleError,
  compareWithBaseline,
  covered,
  matches,
  errorRate,
  isNotItem,
  judgeItems,
  languageOf,
  NOT_ITEM_MATCH,
  overlap,
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

// Lines from the samples' scripts, and items the model wrote for them.
describe("not-items", () => {
  const question = "I want to understand why the old address answered at all.";
  const status = "我昨天把设置页面的开关都接上了";
  const blocked = "我这边卡在签名证书上，申请还没批下来，所以今天没法打包。";
  const opening = "我们先用中文说一下，然后请 Daniel 讲他那边的情况。";

  test("overlap is the share of one text's content units found in the other", () => {
    expect(overlap("Fine with me.", "Fine with me.", "en")).toBe(1);
    expect(overlap("Investigate why the old address answered at all", question, "en")).toBe(0.75);
    expect(overlap(question, "Investigate why the old address answered at all", "en")).toBe(0.6);
    expect(overlap("Ship it.", "我们周五发布。", "mixed")).toBe(0);
  });

  test("a verbatim copy matches", () => {
    expect(isNotItem(question, question, "en")).toBe(true);
    expect(isNotItem("Fine with me.", "Fine with me.", "en")).toBe(true);
    expect(isNotItem(status, status, "zh")).toBe(true);
    expect(isNotItem(opening, opening, "mixed")).toBe(true);
  });

  test("a shortened or lightly changed copy matches", () => {
    expect(isNotItem("Understand why the old address answered at all.", question, "en")).toBe(true);
    expect(isNotItem("Investigate why the old address answered at all", question, "en")).toBe(true);
    expect(isNotItem("昨天设置页面的开关接上了", status, "zh")).toBe(true);
    expect(isNotItem("我昨天把设置页面的开关都接上了，今天先测试", status, "zh")).toBe(true);
    expect(isNotItem("我这边卡在签名证书上，申请还没批下来，所以今天没", blocked, "zh")).toBe(true);
    expect(isNotItem("先用中文说一下，然后请 Daniel 讲情况", opening, "mixed")).toBe(true);
  });

  test("an unrelated item does not match", () => {
    expect(isNotItem("Tessa turns off the default empty file setting.", question, "en")).toBe(
      false,
    );
    expect(isNotItem("打包先往后放一天。", blocked, "zh")).toBe(false);
    expect(isNotItem("Daniel will change the date format this week.", opening, "mixed")).toBe(
      false,
    );
  });

  // Both directions must reach the line. A real item often reuses most of a
  // not-item's words (the problem it solves), or is mostly made of them, but
  // not both: it adds what was decided.
  test("a real decision that reuses words of a not-item does not match", () => {
    const blocking = "但是按钮放在正中间，会挡住下面的录音列表。";
    const decision = "按钮放在中间，列表往下移一点。";
    expect(overlap(decision, blocking, "zh")).toBeGreaterThanOrEqual(NOT_ITEM_MATCH);
    expect(isNotItem(decision, blocking, "zh")).toBe(false);

    const problem = "The screenshots in the store were also out of date for a week.";
    const rule = "The store screenshots get updated in the same week as the release.";
    expect(overlap(problem, rule, "en")).toBeGreaterThanOrEqual(NOT_ITEM_MATCH);
    expect(isNotItem(rule, problem, "en")).toBe(false);

    const request =
      "On my side, users in London are asking for the date to be written the British way in the note title.";
    expect(
      isNotItem("Follow the system setting for the date in the note title.", request, "mixed"),
    ).toBe(false);
  });

  test("a paraphrase is not caught; only copies are", () => {
    expect(isNotItem("我继续跟进签名证书申请的审批进度", blocked, "zh")).toBe(false);
  });
});

describe("judging items", () => {
  const transcript =
    "我们周五发布。Daniel: I think we should test on an older Mac. Fine with me. 王磊周三之前改完。";
  const sample = (language) => ({
    language,
    transcript,
    not_items: ["I think we should test on an older Mac.", "Fine with me."],
  });
  const items = [
    { kind: "decision", text: "我们周五发布。" },
    { kind: "decision", text: "Fine with me." },
    { kind: "action item", text: "Tom buys three new monitors" },
    { kind: "action item", text: "Test on an older Mac" },
  ];

  const limit = "(known limit: status reports and problems listed as decisions or tasks)";
  const notItemWarnings = [
    `Not a decision (said in the meeting, never agreed or taken on): "Fine with me." matches "Fine with me." ${limit}`,
    `Not an action item (said in the meeting, never agreed or taken on): "Test on an older Mac" matches "I think we should test on an older Mac." ${limit}`,
  ];

  test("unsupported items fail a Chinese or English sample, and not-items warn", () => {
    const { problems, warnings, notItems } = judgeItems(items, sample("en"));
    expect(problems).toEqual(["Unsupported action item (0.0% said): Tom buys three new monitors"]);
    expect(warnings).toEqual([]);
    expect(notItems).toEqual(notItemWarnings);
  });

  test("in a mixed sample unsupported items and not-items both warn", () => {
    const { problems, warnings, notItems } = judgeItems(items, sample("mixed"));
    expect(problems).toEqual([]);
    expect(warnings).toEqual([
      "Unsupported action item (0.0% said): Tom buys three new monitors (known mixed-language limit)",
    ]);
    expect(notItems).toEqual(notItemWarnings);
  });

  test("each item keeps its support for the report", () => {
    const judged = judgeItems(items, sample("zh"));
    expect(judged.items.map((item) => item.support)).toEqual([
      1,
      1,
      support("Tom buys three new monitors", transcript, "zh"),
      support("Test on an older Mac", transcript, "zh"),
    ]);
  });

  test("a sample without not-items is judged on support alone", () => {
    const { problems } = judgeItems(items.slice(0, 1), { language: "zh", transcript });
    expect(problems).toEqual([]);
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
