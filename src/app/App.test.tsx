import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { NoteView, Recording } from "../features/library/library";
import { App } from "./App";

beforeEach(() => {
  // Day headings are relative to today.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 8, 26, 15, 0));
});

afterEach(async () => {
  // Unmount and let the Models screen stop listening before the mocks go away.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  vi.useRealTimers();
});

const note: NoteView = {
  source: "manual",
  inputs: ["microphone", "computer audio"],
  sections: [
    { heading: "Summary", body: "The team reviewed the October release." },
    { heading: "Decisions", body: "- Ship on Wednesday." },
    { heading: "Action items", body: "- Send the checklist." },
    { heading: "Transcript", body: "00:00:04 Okay, let's start." },
    { heading: "Audio", body: "[audio.wav](audio.wav)" },
  ],
};

// Deliberately out of order: the list sorts newest first itself.
const recordings: Recording[] = [
  {
    folder: "2026-09-25-1645",
    start: "2026-09-25T16:45",
    duration_seconds: 27 * 60,
    status: "ready",
  },
  { folder: "2026-09-26-1130", start: "2026-09-26T11:30", duration_seconds: 1080, status: "saved" },
  {
    folder: "2026-09-26-0905",
    start: "2026-09-26T09:05",
    duration_seconds: 3981,
    status: "failed",
    reason: "Not enough memory",
  },
  {
    folder: "2026-09-26-1410",
    start: "2026-09-26T14:10",
    duration_seconds: 42 * 60,
    status: "working",
  },
  {
    folder: "2026-09-26-1200",
    start: "2026-09-26T12:00",
    duration_seconds: 60,
    status: "needs_models",
  },
];

type Call = { cmd: string; args: unknown };

// A fake library. `move_to_trash` removes the folder, like the real one.
function fakeLibrary(initial: Recording[]) {
  let list = [...initial];
  const calls: Call[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const folder = (args as { folder?: string } | undefined)?.folder;
      switch (cmd) {
        case "list_recordings":
          return list;
        case "read_note":
          return note;
        case "show_in_finder":
          return null;
        case "move_to_trash":
          list = list.filter((item) => item.folder !== folder);
          return null;
        case "list_models":
          return { memory_bytes: 16 * 1024 ** 3, models: [] };
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

function rows() {
  const nav = screen.getByRole("navigation", { name: "Recordings" });
  return within(nav)
    .getAllByRole("button")
    .map((row) => row.textContent);
}

test("an empty library says so and still offers Record", async () => {
  const calls = fakeLibrary([]);
  render(<App />);

  expect(await screen.findByText("No recordings yet")).toBeInTheDocument();
  expect(screen.getByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Record" })).toHaveLength(2);
  expect(screen.getByRole("button", { name: "Models" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Settings" })).toBeInTheDocument();
  expect(calls.map(({ cmd }) => cmd)).toEqual(["list_recordings"]);
});

test("Record stays unavailable until recording is built", async () => {
  fakeLibrary([]);
  render(<App />);
  await screen.findByText("No recordings yet");
  for (const button of screen.getAllByRole("button", { name: "Record" })) {
    expect(button).toBeDisabled();
  }
});

test("the list shows newest first, grouped by day, with time, duration, and status", async () => {
  fakeLibrary(recordings);
  render(<App />);
  await screen.findByText("14:10");

  expect(rows()).toEqual([
    "14:1042 minWorking",
    "12:001 minNeeds models",
    "11:3018 minSaved",
    "09:051 h 06 minFailed· Not enough memory",
    "16:4527 minReady",
  ]);
  const days = within(screen.getByRole("navigation", { name: "Recordings" })).getAllByRole(
    "heading",
  );
  expect(days.map((day) => day.textContent)).toEqual(["Today", "Yesterday"]);
});

test("selecting a ready recording shows its note", async () => {
  const calls = fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^16:45/ }));

  expect(await screen.findByRole("heading", { name: "2026-09-25 16:45" })).toBeInTheDocument();
  expect(screen.getByText("27 min · Manual · Microphone, computer audio")).toBeInTheDocument();
  expect(screen.getByText("The team reviewed the October release.")).toBeInTheDocument();
  expect(screen.getByText("Ship on Wednesday.")).toBeInTheDocument();
  expect(screen.getByText("Okay, let's start.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^16:45/ })).toHaveAttribute("aria-current", "true");
  expect(calls).toContainEqual({ cmd: "read_note", args: { folder: "2026-09-25-1645" } });
});

test("a failed recording shows why, and the audio is kept", async () => {
  fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^09:05/ }));

  expect(await screen.findByText("Not enough memory", { selector: "p" })).toBeInTheDocument();
  expect(screen.getByText("The audio is saved and no note was written.")).toBeInTheDocument();
});

test("Needs models opens the Models screen", async () => {
  fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^12:00/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Download models" }));

  expect(await screen.findByRole("heading", { name: "Models" })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Models" })).toHaveAttribute("aria-pressed", "true");
});

test("Show in Finder reveals the recording folder", async () => {
  const calls = fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));
  fireEvent.click(await screen.findByRole("button", { name: "More actions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Show in Finder" }));

  expect(calls).toContainEqual({ cmd: "show_in_finder", args: { folder: "2026-09-26-1130" } });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("Move to Trash moves the folder, updates the list, and goes back home", async () => {
  const calls = fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));
  fireEvent.click(await screen.findByRole("button", { name: "More actions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Move to Trash" }));

  expect(await screen.findByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
  expect(calls).toContainEqual({ cmd: "move_to_trash", args: { folder: "2026-09-26-1130" } });
  expect(rows()).not.toContain("11:3018 minSaved");
  expect(rows()).toHaveLength(4);
});

test("the more actions menu closes with Escape", async () => {
  fakeLibrary(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));
  fireEvent.click(await screen.findByRole("button", { name: "More actions" }));
  expect(screen.getByRole("menu")).toBeInTheDocument();

  fireEvent.keyDown(screen.getByRole("menu"), { key: "Escape" });

  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

test("the Models button opens the Models screen and closes it again", async () => {
  mockIPC(
    (cmd) => {
      if (cmd === "list_recordings") return [];
      if (cmd === "list_models") return { memory_bytes: 16 * 1024 ** 3, models: [] };
    },
    { shouldMockEvents: true },
  );
  render(<App />);
  const button = await screen.findByRole("button", { name: "Models" });
  expect(button).toHaveAttribute("aria-pressed", "false");

  fireEvent.click(button);

  expect(await screen.findByRole("heading", { name: "Models" })).toBeInTheDocument();
  expect(await screen.findByText("This Mac has 16 GB of memory.")).toBeInTheDocument();
  expect(button).toHaveAttribute("aria-pressed", "true");

  fireEvent.click(button);
  expect(screen.queryByRole("heading", { name: "Models" })).not.toBeInTheDocument();
});
