import { describe, expect, test } from "vitest";
import {
  compareWithBaseline,
  covered,
  errorRate,
  languageOf,
  support,
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
