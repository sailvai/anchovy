import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { Recording } from "../features/library/library";
import type { MeetingPrompt } from "../ipc/meetings";
import { App } from "./App";

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 8, 26, 15, 0));
});

afterEach(async () => {
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  vi.useRealTimers();
});

const notesFolder = {
  path: "/Users/someone/Documents/Anchovy",
  display: "~/Documents/Anchovy",
  exists: true,
  obsidian_vault: false,
};

const zoom: MeetingPrompt = {
  id: 1,
  app: "zoom",
  headline: "Zoom meeting started.",
  body: "Record it? Anchovy records only if you choose Record.",
};

const earlier: Recording = {
  folder: "2026-09-25-1645",
  start: "2026-09-25T16:45",
  duration_seconds: 27 * 60,
  status: "ready",
};

const started = {
  folder: "/Users/someone/Documents/Anchovy/2026-09-26-1500",
  microphone: "MacBook Air Microphone",
  computer_audio: "recording",
};

type Call = { cmd: string; args: unknown };

// The window with one earlier recording. `waiting` is the prompt Rust has
// when the window opens; `answer` is what answer_meeting_prompt returns.
function fakeWindow({ waiting = null as MeetingPrompt | null, answer = (): unknown => null } = {}) {
  const calls: Call[] = [];
  let list: Recording[] = [earlier];
  mockIPC(
    (cmd, args) => {
      calls.push({ cmd, args });
      switch (cmd) {
        case "setup_status":
          return {
            notes_folder: notesFolder,
            default_folder: notesFolder,
            microphone: "allowed",
            computer_audio: "allowed",
            finished: true,
            can_record: true,
          };
        case "recording_sources":
          return { microphone: "MacBook Air Microphone", computer_audio: null };
        case "check_computer_audio":
          return "allowed";
        case "list_recordings":
          return list;
        case "read_note":
          return { source: "manual", inputs: ["microphone"], sections: [] };
        case "meeting_prompt":
          return waiting;
        case "answer_meeting_prompt": {
          const result = answer();
          if (result) {
            list = [
              {
                folder: "2026-09-26-1500",
                start: "2026-09-26T15:00",
                duration_seconds: null,
                status: "recording",
              },
              earlier,
            ];
          }
          return result;
        }
      }
    },
    { shouldMockEvents: true },
  );
  return calls;
}

function banner() {
  return screen.queryByRole("status", { name: "Meeting prompt" });
}

async function openWindow() {
  render(<App />);
  expect(await screen.findByRole("heading", { name: "Ready to record" })).toBeInTheDocument();
  // Listening for events is asynchronous; let it finish before emitting.
  await act(() => new Promise((resolve) => setTimeout(resolve, 0)));
}

test("a meeting shows the prompt at the top with Record and Not now", async () => {
  fakeWindow();
  await openWindow();
  expect(banner()).not.toBeInTheDocument();

  await act(() => emit("meeting-prompt", zoom));

  expect(banner()).toHaveTextContent(
    "Zoom meeting started. Record it? Anchovy records only if you choose Record.",
  );
  expect(screen.getByRole("button", { name: "Not now" })).toBeEnabled();
  // The banner's, next to the sidebar's and the home pane's.
  expect(screen.getAllByRole("button", { name: "Record" })).toHaveLength(3);
});

test("a browser prompt names the browser", async () => {
  fakeWindow();
  await openWindow();

  await act(() =>
    emit("meeting-prompt", {
      ...zoom,
      app: "chrome",
      headline: "A call may have started in Chrome.",
    }),
  );

  expect(banner()).toHaveTextContent("A call may have started in Chrome.");
});

test("a prompt waiting when the window opens is shown", async () => {
  fakeWindow({ waiting: zoom });
  await openWindow();
  expect(await screen.findByRole("status", { name: "Meeting prompt" })).toBeInTheDocument();
});

test("the prompt does not block the window", async () => {
  fakeWindow();
  await openWindow();
  await act(() => emit("meeting-prompt", zoom));

  fireEvent.click(screen.getByRole("button", { name: /^16:45/ }));

  expect(await screen.findByRole("heading", { name: "2026-09-25 16:45" })).toBeInTheDocument();
  expect(banner()).toBeInTheDocument();
});

test("Not now answers the prompt and hides it", async () => {
  const calls = fakeWindow();
  await openWindow();
  await act(() => emit("meeting-prompt", zoom));

  await act(async () => fireEvent.click(screen.getByRole("button", { name: "Not now" })));

  expect(banner()).not.toBeInTheDocument();
  expect(calls).toContainEqual({
    cmd: "answer_meeting_prompt",
    args: { id: 1, answer: "not_now" },
  });
  expect(calls.map(({ cmd }) => cmd)).not.toContain("start_recording");
});

test("Record in the banner starts recording through the prompt", async () => {
  const calls = fakeWindow({ answer: () => started });
  await openWindow();
  await act(() => emit("meeting-prompt", zoom));

  const record = screen.getAllByRole("button", { name: "Record" })[0];
  fireEvent.click(record);

  expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  expect(banner()).not.toBeInTheDocument();
  expect(calls).toContainEqual({
    cmd: "answer_meeting_prompt",
    args: { id: 1, answer: "record" },
  });
  // Manual Record is a different command, written as source: manual.
  expect(calls.map(({ cmd }) => cmd)).not.toContain("start_recording");
});

test("Record from the notification shows the recording in the window", async () => {
  fakeWindow();
  await openWindow();
  await act(() => emit("meeting-prompt", zoom));

  await act(() => emit("meeting-prompt", null));
  await act(() => emit("recording-started", started));

  expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
  expect(banner()).not.toBeInTheDocument();
});

test("a Record from the notification that fails says why", async () => {
  fakeWindow();
  await openWindow();

  await act(() => emit("meeting-record-failed", "Anchovy needs microphone access to record."));

  expect(
    screen.getByText(
      "Anchovy couldn't start recording. Anchovy needs microphone access to record.",
    ),
  ).toBeInTheDocument();
});

test("the prompt goes away when the meeting ends", async () => {
  fakeWindow();
  await openWindow();
  await act(() => emit("meeting-prompt", zoom));
  expect(banner()).toBeInTheDocument();

  await act(() => emit("meeting-prompt", null));

  expect(banner()).not.toBeInTheDocument();
});
