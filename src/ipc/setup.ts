import { invoke } from "@tauri-apps/api/core";

// Mirrors `Access`, `SetupStatus`, and `NotesFolder` in src-tauri/src/setup.rs
// and src-tauri/src/folder_access.rs.
export type Access = "not_asked" | "unchecked" | "allowed" | "denied";

export type NotesFolder = {
  path: string;
  // The path with the home folder written as ~.
  display: string;
  exists: boolean;
  obsidian_vault: boolean;
};

export type SetupStatus = {
  notes_folder: NotesFolder | null;
  default_folder: NotesFolder;
  microphone: Access;
  computer_audio: Access;
  finished: boolean;
  can_record: boolean;
};

export type PrivacyPane = "microphone" | "computer_audio";

export function setupStatus(): Promise<SetupStatus> {
  return invoke<SetupStatus>("setup_status");
}

// Both resolve to null when the user cancels the panel.
export function chooseNotesFolder(): Promise<NotesFolder | null> {
  return invoke<NotesFolder | null>("choose_notes_folder");
}

export function chooseDefaultNotesFolder(): Promise<NotesFolder | null> {
  return invoke<NotesFolder | null>("use_default_notes_folder");
}

export function requestMicrophone(): Promise<Access> {
  return invoke<Access>("request_microphone");
}

// `ask` is the Allow button: the first check shows the system prompt. Takes
// about a second.
export function checkComputerAudio(ask: boolean): Promise<Access> {
  return invoke<Access>("check_computer_audio", { ask });
}

export function finishSetup(): Promise<void> {
  return invoke("finish_setup");
}

export function openPrivacySettings(pane: PrivacyPane): Promise<void> {
  return invoke("open_privacy_settings", { pane });
}
