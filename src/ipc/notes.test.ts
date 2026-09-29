import { emit } from "@tauri-apps/api/event";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, expect, test } from "vitest";
import {
  generateNote,
  noteProgress,
  noteSettings,
  onNoteProgress,
  onNotesChanged,
  resumeWaitingNotes,
  type NoteProgress,
} from "./notes";

afterEach(() => clearMocks());

test("commands use the Rust names and arguments", async () => {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "note_settings") return { generate_notes_automatically: false };
    if (cmd === "note_progress") return {};
    return null;
  });

  await generateNote("2026-09-26-1410", true);
  await noteProgress();
  expect(await noteSettings()).toEqual({ generate_notes_automatically: false });
  await resumeWaitingNotes();

  expect(calls).toEqual([
    { cmd: "generate_note", args: { folder: "2026-09-26-1410", replace: true } },
    { cmd: "note_progress", args: {} },
    { cmd: "note_settings", args: {} },
    { cmd: "resume_waiting_notes", args: {} },
  ]);
});

test("progress and changes arrive as events", async () => {
  mockIPC(() => null, { shouldMockEvents: true });
  const progress: NoteProgress[] = [];
  const changed: string[] = [];
  const stop = await onNoteProgress((p) => progress.push(p));
  const stopChanged = await onNotesChanged((folder) => changed.push(folder));

  const payload = {
    folder: "2026-09-26-1410",
    stage: "transcribing",
    done_seconds: 30,
    total_seconds: 70,
  };
  await emit("note-progress", payload);
  await emit("notes-changed", "2026-09-26-1410");
  stop();
  stopChanged();

  expect(progress).toEqual([payload]);
  expect(changed).toEqual(["2026-09-26-1410"]);
});
