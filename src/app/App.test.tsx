import { emit } from "@tauri-apps/api/event";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { NoteView, Recording } from "../features/library/library";
import type { SetupStatus } from "../ipc/setup";
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

const notesFolder = {
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: true,
  obsidian_vault: false,
};

// First launch done: a notes folder, the microphone and computer audio allowed.
const ready: SetupStatus = {
  notes_folder: notesFolder,
  default_folder: notesFolder,
  microphone: "allowed",
  computer_audio: "allowed",
  finished: true,
  can_record: true,
};

// Answers for the commands every screen of the window uses.
function windowCommand(cmd: string, setup: SetupStatus) {
  switch (cmd) {
    case "setup_status":
      return setup;
    case "recording_sources":
      return { microphone: "MacBook Air Microphone", computer_audio: null };
    case "check_computer_audio":
      return setup.computer_audio;
    case "list_models":
      return { memory_bytes: 16 * 1024 ** 3, models: [] };
  }
}

// A fake library. `move_to_trash` removes the folder, like the real one.
function fakeLibrary(initial: Recording[], setup: SetupStatus = ready) {
  let list = [...initial];
  const calls: Call[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const folder = (args as { folder?: string } | undefined)?.folder;
      const shared = windowCommand(cmd, setup);
      if (shared !== undefined) return shared;
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
  expect(new Set(calls.map(({ cmd }) => cmd))).toEqual(
    new Set([
      "setup_status",
      "list_recordings",
      "recording_sources",
      "check_computer_audio",
      "meeting_prompt",
    ]),
  );
});

test("Record is available once there is a notes folder and the microphone is allowed", async () => {
  fakeLibrary([]);
  render(<App />);
  await screen.findByText("No recordings yet");
  for (const button of screen.getAllByRole("button", { name: "Record" })) {
    expect(button).toBeEnabled();
  }
});

test("Record is disabled without microphone permission, and says why", async () => {
  fakeLibrary([], { ...ready, microphone: "denied", can_record: false });
  render(<App />);
  await screen.findByText("No recordings yet");
  const buttons = screen.getAllByRole("button", { name: "Record" });
  expect(buttons).toHaveLength(2);
  for (const button of buttons) expect(button).toBeDisabled();
  expect(
    screen.getByText("Anchovy needs microphone access to record. Allow it in System Settings."),
  ).toBeInTheDocument();
});

test("Record is disabled with no notes folder: first launch asks for one", async () => {
  const calls = fakeLibrary([], { ...ready, notes_folder: null, can_record: false });
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Choose a notes folder" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Record" })).not.toBeInTheDocument();
  expect(calls.map(({ cmd }) => cmd)).not.toContain("start_recording");
});

test("computer audio that is not allowed says what is missing and offers Allow", async () => {
  fakeLibrary([], { ...ready, computer_audio: "denied" });
  render(<App />);
  expect(await screen.findByText("Not allowed")).toBeInTheDocument();
  expect(screen.getByText("MacBook Air Microphone")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Allow" })).toBeInTheDocument();
  expect(
    screen.getByText(/The other side of online meetings will not be in the recording/),
  ).toBeInTheDocument();
});

test("allowed computer audio shows as recorded, next to the microphone's name", async () => {
  fakeLibrary([]);
  render(<App />);
  expect(await screen.findByText("Will be recorded")).toBeInTheDocument();
  expect(screen.getByText("MacBook Air Microphone")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Allow" })).not.toBeInTheDocument();
});

const defaultModel = {
  id: "qwen3-asr-1.7b",
  role: "transcribe",
  display_name: "Qwen3-ASR 1.7B",
  languages: [],
  size_bytes: 3.4e9,
  min_ram_gb: 8,
  license: "Apache-2.0",
  fit: "recommended",
  selected: true,
  state: { kind: "not_downloaded" },
};

// First launch, stopped at the models screen: folder chosen, microphone
// allowed, computer audio denied. Record starts a recording that grows until
// Stop saves it.
function fakeFirstLaunchThenRecording() {
  const live = "/Users/someone/Documents/Anchovy/2026-09-26-1502";
  let setup: SetupStatus = {
    ...ready,
    computer_audio: "denied",
    finished: false,
  };
  let list: Recording[] = [];
  const calls: Call[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "list_models") return { memory_bytes: 16 * 1024 ** 3, models: [defaultModel] };
      const shared = windowCommand(cmd, setup);
      if (shared !== undefined) return shared;
      switch (cmd) {
        case "list_recordings":
          return list;
        case "finish_setup":
          setup = { ...setup, finished: true };
          return null;
        case "start_recording":
          list = [
            {
              folder: "2026-09-26-1502",
              start: "2026-09-26T15:02",
              duration_seconds: null,
              status: "recording",
            },
          ];
          return {
            folder: live,
            microphone: "MacBook Air Microphone",
            computer_audio: "not_allowed",
          };
        case "stop_recording":
          list = [{ ...list[0], duration_seconds: 3, status: "saved" }];
          return {
            folder: live,
            audio: `${live}/audio.wav`,
            seconds: 3,
            bytes: 288_044,
            inputs: ["microphone"],
            levels: { microphone: { peak: 0.2, rms: 0.05 }, computer: null },
            dropped_frames: 0,
          };
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

test("after Later, Record still records, shows progress, and Stop saves it", async () => {
  const calls = fakeFirstLaunchThenRecording();
  render(<App />);

  expect(await screen.findByRole("heading", { name: "Download the models" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Later" }));
  expect(await screen.findByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
  expect(calls.map(({ cmd }) => cmd)).toContain("finish_setup");
  expect(calls.map(({ cmd }) => cmd)).not.toContain("download_model");

  const [record] = screen.getAllByRole("button", { name: "Record" });
  expect(record).toBeEnabled();
  fireEvent.click(record);

  const stop = await screen.findByRole("button", { name: "Stop" });
  expect(screen.getByLabelText("Elapsed time")).toHaveTextContent("00:00:00");
  expect(screen.getByText("MacBook Air Microphone")).toBeInTheDocument();
  expect(screen.getByText("Not allowed")).toBeInTheDocument();
  await act(() => emit("recording-progress", { seconds: 2.4, bytes: 230_444 }));
  expect(screen.getByLabelText("Elapsed time")).toHaveTextContent("00:00:02");
  expect(screen.getByText("0.2 MB · WAV, 48 kHz, 16-bit, mono")).toBeInTheDocument();
  for (const button of screen.getAllByRole("button", { name: "Record" })) {
    expect(button).toBeDisabled();
  }

  fireEvent.click(stop);

  expect(await screen.findByRole("heading", { name: "2026-09-26 15:02" })).toBeInTheDocument();
  expect(calls.map(({ cmd }) => cmd)).toContain("stop_recording");
  expect(rows()).toEqual(["15:023 sSaved"]);
  expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
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
      return windowCommand(cmd, ready);
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

// --- Notes: Generate note, Working, Needs models, Failed, Retry, Regenerate

const asrModel = {
  ...defaultModel,
  state: { kind: "ready" },
};
const summaryModel = {
  ...defaultModel,
  id: "qwen3-4b-instruct-2507-q4",
  role: "summarize",
  display_name: "Qwen3-4B-Instruct-2507",
  size_bytes: 2.3e9,
  state: { kind: "not_downloaded" },
};

// A fake library whose notes go through generate_note like the real
// pipeline: the recording turns Working, and notes-changed says so.
function fakeNotes(
  initial: Recording[],
  {
    automatic = true,
    models = [asrModel, { ...summaryModel, state: { kind: "ready" } }],
    generateError = null as string | null,
    progress = {} as Record<string, unknown>,
  } = {},
) {
  let list = [...initial];
  const calls: Call[] = [];
  // Changes a recording's status, as the pipeline would.
  const setStatus = (folder: string, status: "ready") => {
    list = list.map((item) => (item.folder === folder ? { ...item, status } : item));
  };
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const folder = (args as { folder?: string } | undefined)?.folder;
      if (cmd === "list_models") return { memory_bytes: 16 * 1024 ** 3, models };
      const shared = windowCommand(cmd, ready);
      if (shared !== undefined) return shared;
      switch (cmd) {
        case "list_recordings":
          return list;
        case "read_note":
          return note;
        case "get_settings":
          return {
            input_device: null,
            recording_quality: "high",
            generate_notes_automatically: automatic,
          };
        case "note_progress":
          return progress;
        case "resume_waiting_notes":
          return null;
        case "generate_note":
          if (generateError) throw generateError;
          list = list.map((item) =>
            item.folder === folder ? { ...item, status: "working" as const } : item,
          );
          return null;
      }
    },
    { shouldMockEvents: true },
  );
  return Object.assign(calls, { setStatus });
}

const generateCalls = (calls: Call[]) => calls.filter(({ cmd }) => cmd === "generate_note");

test("with automatic notes off, a saved recording offers Generate note", async () => {
  const calls = fakeNotes(recordings, { automatic: false });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));

  expect(await screen.findByText("No note yet")).toBeInTheDocument();
  expect(
    await screen.findByText(
      "Automatic notes are off. Choose Generate note to write one on this Mac.",
    ),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Generate note" }));

  expect(await screen.findByText("Writing the note")).toBeInTheDocument();
  expect(generateCalls(calls)).toEqual([
    { cmd: "generate_note", args: { folder: "2026-09-26-1130", replace: false } },
  ]);
  expect(rows()).toContain("11:3018 minWorking");
});

test("while Working, the pane and the list show which step is running", async () => {
  fakeNotes(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^14:10/ }));
  expect(await screen.findByText("Writing the note")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Generate note" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();

  await act(() =>
    emit("note-progress", {
      folder: "2026-09-26-1410",
      stage: "transcribing",
      done_seconds: 1620,
      total_seconds: 2520,
    }),
  );
  expect(
    screen.getByText("Step 1 of 2. The audio is saved; the note appears here when it is ready."),
  ).toBeInTheDocument();
  expect(screen.getByText("27 of 42 min")).toBeInTheDocument();
  expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "64");
  expect(rows()[0]).toBe("14:1042 minWorking· Transcribing");

  await act(() =>
    emit("note-progress", {
      folder: "2026-09-26-1410",
      stage: "summarizing",
      done: 1,
      total: 3,
    }),
  );
  expect(
    screen.getByText("Step 2 of 2. The audio is saved; the note appears here when it is ready."),
  ).toBeInTheDocument();
  expect(screen.getByText("Part 2 of 3")).toBeInTheDocument();
  expect(rows()[0]).toBe("14:1042 minWorking· Writing summary");
});

test("a recording that turns Ready shows its note without reselecting it", async () => {
  const fake = fakeNotes(recordings, { automatic: false });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));
  fireEvent.click(await screen.findByRole("button", { name: "Generate note" }));
  expect(await screen.findByText("Writing the note")).toBeInTheDocument();

  // The pipeline finished.
  fake.setStatus("2026-09-26-1130", "ready");
  await act(() => emit("notes-changed", "2026-09-26-1130"));

  expect(await screen.findByText("The team reviewed the October release.")).toBeInTheDocument();
});

test("Needs models names the missing model and what is already here", async () => {
  fakeNotes(recordings, { models: [asrModel, summaryModel] });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^12:00/ }));

  expect(await screen.findByText("The summary model is not on this Mac")).toBeInTheDocument();
  expect(
    screen.getByText("Anchovy starts the note as soon as the download finishes."),
  ).toBeInTheDocument();
  expect(screen.getByText("On this Mac")).toBeInTheDocument();
  expect(screen.getByText("2.3 GB to download")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Download models" })).toBeInTheDocument();
});

test("a finished download starts the notes that waited for it", async () => {
  const calls = fakeNotes(recordings, { models: [asrModel, summaryModel] });
  render(<App />);
  await screen.findByText("14:10");

  await act(() => emit("models-changed", "qwen3-4b-instruct-2507-q4"));

  expect(calls.map(({ cmd }) => cmd)).toContain("resume_waiting_notes");
});

test("Failed shows the reason and Retry starts the note again", async () => {
  const failed: Recording[] = [
    {
      folder: "2026-09-26-0905",
      start: "2026-09-26T09:05",
      duration_seconds: 3981,
      status: "failed",
      reason:
        "Not enough memory to write the summary. The summary model needs about 7.1 GB of free memory. Close other apps, then choose Retry.",
    },
  ];
  const calls = fakeNotes(failed);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^09:05/ }));

  expect(
    await screen.findByText("Not enough memory to write the summary", { selector: "p" }),
  ).toBeInTheDocument();
  expect(
    screen.getByText(
      "The summary model needs about 7.1 GB of free memory. Close other apps, then choose Retry. The audio is saved and no note was written.",
    ),
  ).toBeInTheDocument();
  expect(rows()).toEqual(["09:051 h 06 minFailed· Not enough memory to write the summary"]);

  fireEvent.click(await screen.findByRole("button", { name: "Retry" }));

  expect(await screen.findByText("Writing the note")).toBeInTheDocument();
  expect(generateCalls(calls)).toEqual([
    { cmd: "generate_note", args: { folder: "2026-09-26-0905", replace: true } },
  ]);
});

test("Failed with a model missing offers Download models instead of Retry", async () => {
  fakeNotes(recordings, { models: [asrModel, summaryModel] });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^09:05/ }));

  expect(await screen.findByRole("button", { name: "Download models" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Retry" })).not.toBeInTheDocument();
});

test("Regenerate note asks before it replaces note.md", async () => {
  const calls = fakeNotes(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^16:45/ }));
  await screen.findByText("The team reviewed the October release.");

  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Regenerate note…" }));
  const dialog = await screen.findByRole("alertdialog", { name: "Replace note.md?" });
  expect(
    await within(dialog).findByText(
      "Anchovy writes a new note with Qwen3-ASR 1.7B and Qwen3-4B-Instruct-2507. Changes you made to this note, for example in Obsidian, will be lost. The audio stays.",
    ),
  ).toBeInTheDocument();

  fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
  expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  expect(generateCalls(calls)).toEqual([]);

  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Regenerate note…" }));
  fireEvent.click(await screen.findByRole("button", { name: "Replace Note" }));

  expect(await screen.findByText("Writing the note")).toBeInTheDocument();
  expect(generateCalls(calls)).toEqual([
    { cmd: "generate_note", args: { folder: "2026-09-25-1645", replace: true } },
  ]);
});

test("a note that cannot start says why", async () => {
  fakeNotes(recordings, {
    generateError: "Download Qwen3-4B-Instruct-2507 in Models first.",
  });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^16:45/ }));
  await screen.findByText("The team reviewed the October release.");
  fireEvent.click(screen.getByRole("button", { name: "More actions" }));
  fireEvent.click(screen.getByRole("menuitem", { name: "Regenerate note…" }));
  fireEvent.click(await screen.findByRole("button", { name: "Replace Note" }));

  expect(await screen.findByRole("alert")).toHaveTextContent(
    "Download Qwen3-4B-Instruct-2507 in Models first.",
  );
  expect(screen.getByText("The team reviewed the October release.")).toBeInTheDocument();
});

test("Regenerate note is only offered for a note that exists", async () => {
  fakeNotes(recordings);
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^11:30/ }));
  fireEvent.click(await screen.findByRole("button", { name: "More actions" }));
  expect(screen.queryByRole("menuitem", { name: "Regenerate note…" })).not.toBeInTheDocument();
});

test("selecting a Working recording asks where its note is", async () => {
  const calls = fakeNotes(recordings, {
    progress: {
      "2026-09-26-1410": { stage: "summarizing", done: 0, total: 1 },
    },
  });
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^14:10/ }));

  expect(
    await screen.findByText(
      "Step 2 of 2. The audio is saved; the note appears here when it is ready.",
    ),
  ).toBeInTheDocument();
  expect(calls.map(({ cmd }) => cmd)).toContain("note_progress");
});

// Two notes folders: the library lists whichever one is current.
function fakeTwoFolders() {
  const vault = {
    path: "/Users/someone/Notes/Vault",
    display: "~/Notes/Vault",
    exists: true,
    obsidian_vault: true,
  };
  let setup: SetupStatus = ready;
  const inVault: Recording[] = [
    {
      folder: "2026-09-26-0800",
      start: "2026-09-26T08:00",
      duration_seconds: 600,
      status: "saved",
    },
  ];
  const calls: Call[] = [];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      const shared = windowCommand(cmd, setup);
      if (shared !== undefined) return shared;
      switch (cmd) {
        case "list_recordings":
          return setup.notes_folder === vault ? inVault : recordings;
        case "read_note":
          return note;
        case "get_settings":
          return {
            input_device: null,
            recording_quality: "high",
            generate_notes_automatically: true,
          };
        case "list_input_devices":
          return [{ uid: "BuiltIn", name: "MacBook Air Microphone", is_default: true }];
        case "choose_notes_folder":
          setup = { ...setup, notes_folder: vault };
          return vault;
        case "start_recording":
          return {
            folder: "/Users/someone/Documents/Anchovy/2026-09-26-1502",
            microphone: "MacBook Air Microphone",
            computer_audio: "recording",
          };
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

test("Settings replaces the right side, like Models, and closes again", async () => {
  fakeTwoFolders();
  render(<App />);
  const button = await screen.findByRole("button", { name: "Settings" });
  expect(button).toBeEnabled();

  fireEvent.click(button);

  expect(await screen.findByRole("heading", { name: "Settings" })).toBeInTheDocument();
  expect(button).toHaveAttribute("aria-pressed", "true");
  expect(screen.queryByRole("heading", { name: "Ready to record" })).not.toBeInTheDocument();
  fireEvent.click(button);
  expect(await screen.findByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
});

test("changing the notes folder rescans the library and clears the selection", async () => {
  const calls = fakeTwoFolders();
  render(<App />);
  fireEvent.click(await screen.findByRole("button", { name: /^16:45/ }));
  expect(await screen.findByRole("heading", { name: "2026-09-25 16:45" })).toBeInTheDocument();

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(await screen.findByRole("button", { name: "Change…" }));

  await waitFor(() => expect(rows()).toEqual(["08:0010 minSaved"]));
  expect(await screen.findByText("~/Notes/Vault")).toBeInTheDocument();
  expect(calls.filter(({ cmd }) => cmd === "list_recordings").length).toBeGreaterThan(1);
  // Nothing is selected, and leaving Settings goes home.
  const nav = screen.getByRole("navigation", { name: "Recordings" });
  expect(within(nav).queryByRole("button", { current: true })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
});

test("Record lets Rust use the saved device, and Change… is off while recording", async () => {
  const calls = fakeTwoFolders();
  render(<App />);
  await screen.findByRole("heading", { name: "Ready to record" });

  fireEvent.click(screen.getAllByRole("button", { name: "Record" })[0]);
  expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  expect(calls.find(({ cmd }) => cmd === "start_recording")?.args).toEqual({});

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  expect(await screen.findByRole("button", { name: "Change…" })).toBeDisabled();
});
