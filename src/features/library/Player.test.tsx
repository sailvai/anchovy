import { clearMocks, mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { clock, type Recording, type Status } from "./library";
import { NotePane } from "./NotePane";
import { Player } from "./Player";

const folder = "2026-09-25-1645";
const audio = { path: `/Users/someone/Documents/Anchovy/${folder}/audio.wav`, name: "audio.wav" };

beforeEach(() => {
  mockConvertFileSrc("macos");
  // jsdom has no media playback.
  vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(async () => {});
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
});

afterEach(async () => {
  // Unmount and let the note stop listening before the mocks go away.
  cleanup();
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  vi.restoreAllMocks();
});

test("times read like the mock: m:ss now, the length with two-digit minutes", () => {
  expect(clock(0)).toBe("0:00");
  expect(clock(65.9)).toBe("1:05");
  expect(clock(1634, true)).toBe("27:14");
  expect(clock(552, true)).toBe("09:12");
  expect(clock(3981, true)).toBe("1:06:21");
  expect(clock(3981)).toBe("1:06:21");
});

test("the player plays the file from the asset protocol and shows its name and length", () => {
  render(<Player audio={audio} seconds={1634} />);

  const element = document.querySelector("audio")!;
  expect(element.getAttribute("src")).toBe(
    "asset://localhost/%2FUsers%2Fsomeone%2FDocuments%2FAnchovy%2F2026-09-25-1645%2Faudio.wav",
  );
  expect(element).toHaveAttribute("preload", "none");
  expect(screen.getByText("audio.wav")).toBeInTheDocument();
  expect(screen.getByLabelText("Current time")).toHaveTextContent("0:00");
  expect(screen.getByLabelText("Length")).toHaveTextContent("27:14");
  expect(screen.getByRole("slider", { name: "Seek" })).toHaveValue("0");
});

test("play and pause, the current time, and seeking", async () => {
  render(<Player audio={audio} seconds={1634} />);
  const element = document.querySelector("audio")!;

  fireEvent.click(screen.getByRole("button", { name: "Play" }));
  expect(HTMLMediaElement.prototype.play).toHaveBeenCalled();
  await act(async () => {
    fireEvent(element, new Event("play"));
  });
  const pause = screen.getByRole("button", { name: "Pause" });

  Object.defineProperty(element, "currentTime", { value: 75.4, writable: true });
  fireEvent(element, new Event("timeupdate"));
  expect(screen.getByLabelText("Current time")).toHaveTextContent("1:15");

  fireEvent.change(screen.getByRole("slider", { name: "Seek" }), { target: { value: "600" } });
  expect(element.currentTime).toBe(600);

  fireEvent.click(pause);
  expect(HTMLMediaElement.prototype.pause).toHaveBeenCalled();
});

test("the length comes from the file once it has loaded", () => {
  render(<Player audio={{ ...audio, name: "audio.m4a" }} seconds={null} />);
  const element = document.querySelector("audio")!;
  Object.defineProperty(element, "duration", { value: 552.3 });

  fireEvent(element, new Event("loadedmetadata"));

  expect(screen.getByLabelText("Length")).toHaveTextContent("09:12");
  expect(screen.getByText("audio.m4a")).toBeInTheDocument();
});

test("a file that cannot be played says so", () => {
  render(<Player audio={audio} seconds={1634} />);

  fireEvent(document.querySelector("audio")!, new Event("error"));

  expect(screen.getByText("Anchovy can't play this file.")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Play" })).toBeDisabled();
});

// --- In the note ------------------------------------------------------------

type Found = { path: string; name: string } | null;

function fakeNote(found: Found) {
  const asked: string[] = [];
  mockIPC(
    (cmd, args) => {
      switch (cmd) {
        case "recording_audio":
          asked.push((args as { folder: string }).folder);
          return found;
        case "list_models":
          return { memory_bytes: 16 * 1024 ** 3, models: [] };
        case "get_settings":
          return {
            input_device: null,
            recording_quality: "high",
            generate_notes_automatically: true,
          };
        case "read_note":
          return { source: "manual", inputs: ["microphone"], sections: [] };
      }
    },
    { shouldMockEvents: true },
  );
  return asked;
}

function renderNote(status: Status) {
  const recording = {
    folder,
    start: "2026-09-25T16:45",
    duration_seconds: 1634,
    status,
    ...(status === "failed" ? { reason: "Not enough memory." } : {}),
  } as Recording;
  render(
    <NotePane
      recording={recording}
      stage={null}
      onGenerate={async () => {}}
      onShowInFinder={async () => {}}
      onMoveToTrash={async () => {}}
      onDownloadModels={() => {}}
    />,
  );
}

for (const status of ["saved", "needs_models", "working", "failed", "ready"] as const) {
  test(`a ${status} recording with audio shows the player above the note`, async () => {
    const asked = fakeNote(audio);
    renderNote(status);

    const player = await screen.findByRole("group", { name: "Audio" });
    expect(player).toHaveTextContent("audio.wav");
    expect(asked).toEqual([folder]);
  });
}

test("the player is hidden while the recording is being recorded", async () => {
  const asked = fakeNote(audio);
  renderNote("recording");

  expect(await screen.findByText("The audio is still being written.")).toBeInTheDocument();
  expect(screen.queryByRole("group", { name: "Audio" })).not.toBeInTheDocument();
  expect(asked).toEqual([]);
});

test("a recording without audio has no player", async () => {
  const asked = fakeNote(null);
  renderNote("saved");

  await screen.findByText("No note yet");
  await act(async () => {});
  expect(asked).toEqual([folder]);
  expect(screen.queryByRole("group", { name: "Audio" })).not.toBeInTheDocument();
});
