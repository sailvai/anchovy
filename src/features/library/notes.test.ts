import { describe, expect, test } from "vitest";
import type { ModelView } from "../../ipc/models";
import {
  headline,
  missingModels,
  missingTitle,
  reasonDetail,
  stageLabel,
  transcribedLabel,
} from "./notes";

describe("failure reasons", () => {
  test("the first sentence is the headline and the rest is the detail", () => {
    const reason =
      "Not enough memory to write the summary. The summary model needs about 7.1 GB of free memory. Close other apps, then choose Retry.";
    expect(headline(reason)).toBe("Not enough memory to write the summary");
    expect(reasonDetail(reason)).toBe(
      "The summary model needs about 7.1 GB of free memory. Close other apps, then choose Retry.",
    );
  });

  test("a one-sentence reason has no detail", () => {
    expect(headline("Not enough memory")).toBe("Not enough memory");
    expect(reasonDetail("Not enough memory")).toBe("");
    expect(headline("No speech was heard in this recording.")).toBe(
      "No speech was heard in this recording",
    );
  });

  test("a decimal point is not the end of a sentence", () => {
    expect(headline("Anchovy couldn't load Qwen3-ASR 1.7B. The file is damaged.")).toBe(
      "Anchovy couldn't load Qwen3-ASR 1.7B",
    );
  });
});

describe("progress", () => {
  test("each stage has a short label for the list", () => {
    expect(stageLabel({ stage: "waiting" })).toBe("Waiting");
    expect(stageLabel({ stage: "transcribing", done_seconds: 1, total_seconds: 2 })).toBe(
      "Transcribing",
    );
    expect(stageLabel({ stage: "summarizing", done: 0, total: 1 })).toBe("Writing summary");
  });

  test("transcription progress is in minutes, or seconds under a minute", () => {
    expect(transcribedLabel(27 * 60 + 40, 42 * 60)).toBe("27 of 42 min");
    expect(transcribedLabel(30, 53)).toBe("30 of 53 s");
    expect(transcribedLabel(0, 90)).toBe("0 of 2 min");
  });
});

describe("missing models", () => {
  const model = (id: string, role: ModelView["role"], ready: boolean): ModelView => ({
    id,
    role,
    display_name: id,
    languages: [],
    size_bytes: 1,
    min_ram_gb: 8,
    license: "Apache-2.0",
    fit: "recommended",
    selected: true,
    state: ready ? { kind: "ready" } : { kind: "not_downloaded" },
  });

  test("only selected models that are not ready are missing", () => {
    const models = [
      model("asr", "transcribe", true),
      model("llm", "summarize", false),
      { ...model("other", "summarize", false), selected: false },
    ];
    expect(missingModels(models).map((m) => m.id)).toEqual(["llm"]);
  });

  test("the title names the missing role", () => {
    expect(missingTitle([model("llm", "summarize", false)])).toBe(
      "The summary model is not on this Mac",
    );
    expect(missingTitle([model("asr", "transcribe", false)])).toBe(
      "The transcription model is not on this Mac",
    );
    expect(
      missingTitle([model("asr", "transcribe", false), model("llm", "summarize", false)]),
    ).toBe("The models for this note are not on this Mac");
    expect(missingTitle([])).toBe("The models for this note are not on this Mac");
  });
});
