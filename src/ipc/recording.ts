import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Quality } from "./settings";

// Mirrors the types in src-tauri/src/recording/core.rs and commands.rs.
export type ComputerAudio = "recording" | "not_allowed";
export type Input = "microphone" | "computer audio";

export type InputDevice = { uid: string; name: string; is_default: boolean };

export type Sources = {
  microphone: string | null;
  // How the last recording in this run went; null before the first one.
  computer_audio: ComputerAudio | null;
};

export type Recording = {
  folder: string;
  microphone: string;
  computer_audio: ComputerAudio;
  // Read from the settings when the recording started.
  quality: Quality;
};

export type Level = { peak: number; rms: number };

export type Saved = {
  folder: string;
  audio: string;
  seconds: number;
  bytes: number;
  inputs: Input[];
  levels: { microphone: Level; computer: Level | null };
  dropped_frames: number;
};

// Pushed once a second while recording. For Small, `bytes` is the WAV being
// written; it becomes audio.m4a when the recording stops.
export type RecordingProgress = { seconds: number; bytes: number };

export function listInputDevices(): Promise<InputDevice[]> {
  return invoke<InputDevice[]>("list_input_devices");
}

// The saved input device, or the system default when it is not connected.
export function recordingSources(): Promise<Sources> {
  return invoke<Sources>("recording_sources");
}

// Rust reads the saved input device and quality when the recording starts.
export function startRecording(): Promise<Recording> {
  return invoke<Recording>("start_recording");
}

export function stopRecording(): Promise<Saved> {
  return invoke<Saved>("stop_recording");
}

// Event name matches `PROGRESS_EVENT` in recording/commands.rs.
export function onRecordingProgress(
  handler: (progress: RecordingProgress) => void,
): Promise<UnlistenFn> {
  return listen<RecordingProgress>("recording-progress", (event) => handler(event.payload));
}

// Every start, including one from the meeting prompt's notification. Matches
// `STARTED_EVENT` in recording/commands.rs.
export function onRecordingStarted(handler: (recording: Recording) => void): Promise<UnlistenFn> {
  return listen<Recording>("recording-started", (event) => handler(event.payload));
}
