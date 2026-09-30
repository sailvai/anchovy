import { invoke } from "@tauri-apps/api/core";

// Mirrors `Settings` in src-tauri/src/settings.rs and `Quality` in
// src-tauri/src/notes/folder.rs. The notes folder is not in here: it is the
// bookmark from first launch, changed with `chooseNotesFolder`.
export type Quality = "high" | "small";

export type Settings = {
  // The chosen input's UID, or null for the system default.
  input_device: string | null;
  recording_quality: Quality;
  generate_notes_automatically: boolean;
};

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

// Resolves to the settings as saved. Applies from the next recording on.
export function updateSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("update_settings", { settings });
}
