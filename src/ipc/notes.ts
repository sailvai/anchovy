import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors `Stage` and `NoteSettings` in src-tauri/src/pipeline.rs.
export type Stage =
  | { stage: "waiting" }
  | { stage: "transcribing"; done_seconds: number; total_seconds: number }
  | { stage: "summarizing"; done: number; total: number };

export type NoteProgress = { folder: string } & Stage;

export type NoteSettings = { generate_notes_automatically: boolean };

// Generate note, Retry, and Regenerate note. `replace` is the answer to
// "Replace note.md?".
export function generateNote(folder: string, replace: boolean): Promise<void> {
  return invoke("generate_note", { folder, replace });
}

// Where each note being written is, by folder name.
export function noteProgress(): Promise<Record<string, Stage>> {
  return invoke<Record<string, Stage>>("note_progress");
}

export function noteSettings(): Promise<NoteSettings> {
  return invoke<NoteSettings>("note_settings");
}

// After a model download: start the notes that waited for it.
export function resumeWaitingNotes(): Promise<void> {
  return invoke("resume_waiting_notes");
}

// Event names match `PROGRESS_EVENT` and `CHANGED_EVENT` in pipeline/commands.rs.
export function onNoteProgress(handler: (progress: NoteProgress) => void): Promise<UnlistenFn> {
  return listen<NoteProgress>("note-progress", (event) => handler(event.payload));
}

export function onNotesChanged(handler: (folder: string) => void): Promise<UnlistenFn> {
  return listen<string>("notes-changed", (event) => handler(event.payload));
}
