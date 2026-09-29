import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Recording } from "./recording";

// Mirrors `Prompt` in src-tauri/src/meetings/detector.rs.
export type MeetingApp = "zoom" | "teams" | "chrome" | "safari" | "edge" | "arc" | "firefox";

export type MeetingPrompt = {
  id: number;
  app: MeetingApp;
  // "Zoom meeting started." or "A call may have started in Chrome."
  headline: string;
  body: string;
};

export type MeetingAnswer = "record" | "not_now";

export function meetingPrompt(): Promise<MeetingPrompt | null> {
  return invoke<MeetingPrompt | null>("meeting_prompt");
}

// Record returns the recording it started; an answer to a prompt that has
// already gone away returns null and records nothing.
export function answerMeetingPrompt(id: number, answer: MeetingAnswer): Promise<Recording | null> {
  return invoke<Recording | null>("answer_meeting_prompt", { id, answer });
}

// Event names match `PROMPT_EVENT` and `RECORD_FAILED_EVENT` in
// meetings/commands.rs. A null prompt means it has gone away.
export function onMeetingPrompt(
  handler: (prompt: MeetingPrompt | null) => void,
): Promise<UnlistenFn> {
  return listen<MeetingPrompt | null>("meeting-prompt", (event) => handler(event.payload));
}

// Record chosen in the notification, but the recording could not start.
export function onMeetingRecordFailed(handler: (reason: string) => void): Promise<UnlistenFn> {
  return listen<string>("meeting-record-failed", (event) => handler(event.payload));
}
