import { describe, expect, test } from "vitest";
import {
  dayLabel,
  folderName,
  formatDuration,
  formatElapsed,
  formatFileSize,
  groupByDay,
  inputsLabel,
  noteBlocks,
  sortNewestFirst,
  startOfFolder,
  statusLabel,
  type Recording,
} from "./library";

function recording(folder: string, rest: Partial<Recording> = {}): Recording {
  const [year, month, day, time] = folder.split("-");
  return {
    folder,
    start: `${year}-${month}-${day}T${time.slice(0, 2)}:${time.slice(2, 4)}`,
    duration_seconds: 60,
    status: "saved",
    ...rest,
  } as Recording;
}

describe("list order", () => {
  test("newest first, across days", () => {
    const list = [
      recording("2026-09-23-0930"),
      recording("2026-09-26-1410"),
      recording("2026-09-25-1645"),
      recording("2026-09-26-0905"),
    ];
    expect(sortNewestFirst(list).map((item) => item.folder)).toEqual([
      "2026-09-26-1410",
      "2026-09-26-0905",
      "2026-09-25-1645",
      "2026-09-23-0930",
    ]);
  });

  test("same minute recordings put the highest number first", () => {
    const list = [
      recording("2026-09-26-1410"),
      recording("2026-09-26-1410-2"),
      recording("2026-09-26-1410-10"),
    ];
    expect(sortNewestFirst(list).map((item) => item.folder)).toEqual([
      "2026-09-26-1410-10",
      "2026-09-26-1410-2",
      "2026-09-26-1410",
    ]);
  });

  test("does not change the list it was given", () => {
    const list = [recording("2026-09-23-0930"), recording("2026-09-26-1410")];
    sortNewestFirst(list);
    expect(list[0].folder).toBe("2026-09-23-0930");
  });
});

describe("status labels", () => {
  test("each status has the label from the spec", () => {
    expect(statusLabel).toEqual({
      recording: "Recording",
      saved: "Saved",
      needs_models: "Needs models",
      working: "Working",
      ready: "Ready",
      failed: "Failed",
    });
  });
});

describe("days", () => {
  const now = new Date(2026, 8, 26, 15, 0);

  test("today, yesterday, then weekday and date", () => {
    expect(dayLabel("2026-09-26T00:05", now)).toBe("Today");
    expect(dayLabel("2026-09-25T23:59", now)).toBe("Yesterday");
    expect(dayLabel("2026-09-23T09:30", now)).toBe("Wed, Sep 23");
  });

  test("other years show the year", () => {
    expect(dayLabel("2025-12-31T10:00", now)).toBe("Wed, Dec 31, 2025");
  });

  test("groups keep the newest first order", () => {
    const groups = groupByDay(
      [
        recording("2026-09-23-0930"),
        recording("2026-09-26-1410"),
        recording("2026-09-23-1520"),
        recording("2026-09-26-0905"),
      ],
      now,
    );
    expect(groups.map(({ day, items }) => [day, items.map((item) => item.folder)])).toEqual([
      ["Today", ["2026-09-26-1410", "2026-09-26-0905"]],
      ["Wed, Sep 23", ["2026-09-23-1520", "2026-09-23-0930"]],
    ]);
  });
});

describe("durations", () => {
  test("minutes, then hours and minutes", () => {
    expect(formatDuration(9 * 60 + 12)).toBe("9 min");
    expect(formatDuration(42 * 60)).toBe("42 min");
    expect(formatDuration(66 * 60 + 21)).toBe("1 h 06 min");
    expect(formatDuration(2 * 3600)).toBe("2 h 00 min");
  });

  test("under a minute shows seconds, unknown shows nothing", () => {
    expect(formatDuration(45)).toBe("45 s");
    expect(formatDuration(null)).toBe("");
  });
});

describe("note text", () => {
  test("inputs say plainly when only the microphone was recorded", () => {
    expect(inputsLabel(["microphone", "computer audio"])).toBe("Microphone, computer audio");
    expect(inputsLabel(["microphone"])).toBe("Microphone only");
    expect(inputsLabel([])).toBe("");
  });

  test("sections split into paragraphs, bullets, and transcript lines", () => {
    expect(noteBlocks("First line\nsame paragraph.\n\nSecond.")).toEqual([
      { kind: "paragraph", text: "First line\nsame paragraph." },
      { kind: "paragraph", text: "Second." },
    ]);
    expect(noteBlocks("- Ship on Wednesday.\n- Keep the old format.")).toEqual([
      { kind: "bullets", items: ["Ship on Wednesday.", "Keep the old format."] },
    ]);
    expect(noteBlocks("-")).toEqual([{ kind: "bullets", items: [] }]);
    expect(noteBlocks("00:00:04 Okay, let's start.\n[00:00:11] It's merged.")).toEqual([
      {
        kind: "transcript",
        lines: [
          { time: "00:00:04", text: "Okay, let's start." },
          { time: "00:00:11", text: "It's merged." },
        ],
      },
    ]);
  });
});

describe("the recording pane", () => {
  test("elapsed time counts hours, minutes, and seconds", () => {
    expect(formatElapsed(0)).toBe("00:00:00");
    expect(formatElapsed(768.4)).toBe("00:12:48");
    expect(formatElapsed(3 * 3600 + 5)).toBe("03:00:05");
  });

  test("file size is in megabytes, like Finder", () => {
    expect(formatFileSize(73_728_044)).toBe("73.7 MB");
    expect(formatFileSize(44)).toBe("0.0 MB");
  });

  test("a recording folder's name gives its start", () => {
    expect(startOfFolder("2026-09-26-1502")).toBe("2026-09-26T15:02");
    expect(startOfFolder("2026-09-26-1502-2")).toBe("2026-09-26T15:02");
    expect(startOfFolder("Notes")).toBeNull();
  });

  test("the folder name is the last part of the path Rust returns", () => {
    expect(folderName("/Users/someone/Documents/Anchovy/2026-09-26-1502")).toBe("2026-09-26-1502");
  });
});
