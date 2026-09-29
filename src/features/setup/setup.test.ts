import { expect, test } from "vitest";
import type { ModelView, ModelsView } from "../../ipc/models";
import type { NotesFolder } from "../../ipc/setup";
import {
  defaultModels,
  downloadInOrder,
  folderNote,
  formatGb,
  timeLeft,
  totalProgress,
} from "./setup";

function model(overrides: Partial<ModelView>): ModelView {
  return {
    id: "m",
    role: "transcribe",
    display_name: "Model",
    languages: [],
    size_bytes: 1e9,
    min_ram_gb: 8,
    license: "Apache-2.0",
    fit: "recommended",
    selected: false,
    state: { kind: "not_downloaded" },
    ...overrides,
  };
}

const view: ModelsView = {
  memory_bytes: 16 * 1024 ** 3,
  models: [
    model({ id: "sum", role: "summarize", selected: true, size_bytes: 2.3e9 }),
    model({ id: "asr-small", role: "transcribe", size_bytes: 1e9 }),
    model({ id: "asr", role: "transcribe", selected: true, size_bytes: 3.4e9 }),
  ],
};

test("the default models are the selected ones, transcription first", () => {
  expect(defaultModels(view).map((m) => m.id)).toEqual(["asr", "sum"]);
});

test("sizes read like the mock", () => {
  expect(formatGb(5.7e9)).toBe("5.7 GB");
  expect(formatGb(0)).toBe("0.0 GB");
});

test("progress counts finished models in full and the rest by bytes so far", () => {
  const models = [
    model({ id: "a", size_bytes: 3.4e9, state: { kind: "ready" } }),
    model({ id: "b", size_bytes: 2.3e9, state: { kind: "downloading", downloaded: 1e9 } }),
  ];
  expect(totalProgress(models, {})).toEqual({ downloaded: 4.4e9, total: 5.7e9 });
  // Live progress events are newer than the list.
  expect(totalProgress(models, { b: 1.5e9 })).toEqual({ downloaded: 4.9e9, total: 5.7e9 });
});

test("time left is rounded up to whole minutes", () => {
  expect(timeLeft(3.6e9, 10e6)).toBe("About 6 minutes left");
  expect(timeLeft(61e6, 1e6)).toBe("About 2 minutes left");
  expect(timeLeft(50e6, 1e6)).toBe("Less than a minute left");
  expect(timeLeft(1e9, 0)).toBeNull();
});

const home = (overrides: Partial<NotesFolder>): NotesFolder => ({
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: false,
  obsidian_vault: false,
  ...overrides,
});

test("the folder row says what happens to the folder", () => {
  expect(folderNote(home({}))).toBe("New folder. Anchovy creates it when you continue.");
  expect(folderNote(home({ exists: true }))).toBe(
    "Existing folder. Each recording gets its own folder inside it.",
  );
  expect(folderNote(home({ exists: true, obsidian_vault: true }))).toBe(
    "Obsidian vault. Each recording gets its own folder inside it.",
  );
});

test("models download one after another, and a failure does not stop the rest", async () => {
  const log: string[] = [];
  await downloadInOrder(["a", "b", "c"], {
    start: async (id) => {
      log.push(`start ${id}`);
      if (id === "b") throw new Error("disk full");
    },
    finished: async (id) => {
      log.push(`done ${id}`);
    },
  });
  expect(log).toEqual(["start a", "done a", "start b", "start c", "done c"]);
});
