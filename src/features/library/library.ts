import { invoke } from "@tauri-apps/api/core";

// Mirrors `Recording` in src-tauri/src/library.rs and `Status` in
// src-tauri/src/notes/state.rs.
export type Status = "recording" | "saved" | "needs_models" | "working" | "ready" | "failed";

export type Recording = {
  folder: string;
  // Local start time, yyyy-MM-ddTHH:mm.
  start: string;
  duration_seconds: number | null;
} & ({ status: Exclude<Status, "failed"> } | { status: "failed"; reason: string });

// Mirrors `NoteView` in src-tauri/src/library.rs.
export type NoteView = {
  source: string | null;
  inputs: string[];
  sections: { heading: string; body: string }[];
};

export function listRecordings(): Promise<Recording[]> {
  return invoke<Recording[]>("list_recordings");
}

export function readNote(folder: string): Promise<NoteView> {
  return invoke<NoteView>("read_note", { folder });
}

export function showInFinder(folder: string): Promise<void> {
  return invoke("show_in_finder", { folder });
}

export function moveToTrash(folder: string): Promise<void> {
  return invoke("move_to_trash", { folder });
}

export const statusLabel: Record<Status, string> = {
  recording: "Recording",
  saved: "Saved",
  needs_models: "Needs models",
  working: "Working",
  ready: "Ready",
  failed: "Failed",
};

const collator = new Intl.Collator("en", { numeric: true });

// Newest first. Recordings started in the same minute are numbered -2, -3, and
// so on; the highest number is the newest.
export function sortNewestFirst(list: Recording[]): Recording[] {
  return [...list].sort(
    (a, b) => b.start.localeCompare(a.start) || collator.compare(b.folder, a.folder),
  );
}

function localDate(start: string) {
  const [year, month, day] = start.slice(0, 10).split("-").map(Number);
  return new Date(year, month - 1, day);
}

export function dayLabel(start: string, now: Date): string {
  const date = localDate(start);
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const days = Math.round((today.getTime() - date.getTime()) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return date.toLocaleDateString("en-US", {
    weekday: "short",
    month: "short",
    day: "numeric",
    year: date.getFullYear() === now.getFullYear() ? undefined : "numeric",
  });
}

export function groupByDay(list: Recording[], now: Date) {
  const groups: { day: string; items: Recording[] }[] = [];
  for (const item of sortNewestFirst(list)) {
    const day = dayLabel(item.start, now);
    const last = groups.at(-1);
    if (last?.day === day) last.items.push(item);
    else groups.push({ day, items: [item] });
  }
  return groups;
}

export function formatDuration(seconds: number | null): string {
  if (seconds === null) return "";
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  return `${Math.floor(minutes / 60)} h ${String(minutes % 60).padStart(2, "0")} min`;
}

// "2026-09-25T16:45" as the note title shows it.
export function startTitle(start: string): string {
  return start.replace("T", " ");
}

export function timeOfDay(start: string): string {
  return start.slice(11, 16);
}

function capitalize(text: string) {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function sourceLabel(source: string | null): string {
  return source ? capitalize(source) : "";
}

// Says plainly when the other side of a call was not recorded.
export function inputsLabel(inputs: string[]): string {
  if (inputs.length === 1 && inputs[0] === "microphone") return "Microphone only";
  return capitalize(inputs.join(", "));
}

export type NoteBlock =
  | { kind: "paragraph"; text: string }
  | { kind: "bullets"; items: string[] }
  | { kind: "transcript"; lines: { time: string; text: string }[] };

const timestamp = /^\[?(\d{1,2}:\d{2}(?::\d{2})?)\]?\s+(.*)$/;

// Splits a note section into blocks at blank lines. The user may have edited
// note.md, so anything that is not a list or a transcript is a paragraph.
export function noteBlocks(body: string): NoteBlock[] {
  return body
    .split(/\n\s*\n/)
    .map((block) => block.trim())
    .filter(Boolean)
    .map((block): NoteBlock => {
      const lines = block.split("\n").map((line) => line.trim());
      if (lines.every((line) => line === "-" || line.startsWith("- "))) {
        return { kind: "bullets", items: lines.map((line) => line.slice(2)).filter(Boolean) };
      }
      const matches = lines.map((line) => timestamp.exec(line));
      if (matches.every(Boolean)) {
        return {
          kind: "transcript",
          lines: matches.map((match) => ({ time: match![1], text: match![2] })),
        };
      }
      return { kind: "paragraph", text: block };
    });
}

// Elapsed recording time as hh:mm:ss.
export function formatElapsed(seconds: number): string {
  const whole = Math.floor(seconds);
  return [Math.floor(whole / 3600), Math.floor(whole / 60) % 60, whole % 60]
    .map((part) => String(part).padStart(2, "0"))
    .join(":");
}

// Decimal megabytes, as Finder shows file sizes.
export function formatFileSize(bytes: number): string {
  return `${(bytes / 1e6).toFixed(1)} MB`;
}

// "2026-09-26-1502" (maybe with "-2") as "2026-09-26T15:02".
export function startOfFolder(folder: string): string | null {
  const match = /^(\d{4}-\d{2}-\d{2})-(\d{2})(\d{2})(-\d+)?$/.exec(folder);
  return match ? `${match[1]}T${match[2]}:${match[3]}` : null;
}

// Rust returns full paths; the library knows recordings by folder name.
export function folderName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? path;
}
