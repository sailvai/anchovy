import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors `ModelsView`, `ModelView`, and `ModelState` in src-tauri/src/models/mod.rs.
export type Role = "transcribe" | "summarize";
export type Fit = "recommended" | "fits" | "too_large";

export type ModelState =
  | { kind: "not_downloaded" }
  | { kind: "paused"; downloaded: number }
  | { kind: "downloading"; downloaded: number }
  | { kind: "failed"; downloaded: number; error: string }
  | { kind: "ready" };

export type ModelView = {
  id: string;
  role: Role;
  display_name: string;
  languages: string[];
  size_bytes: number;
  min_ram_gb: number;
  license: string;
  fit: Fit;
  selected: boolean;
  state: ModelState;
};

export type ModelsView = {
  memory_bytes: number;
  models: ModelView[];
};

export type ModelProgress = { id: string; downloaded: number };

export function listModels(): Promise<ModelsView> {
  return invoke<ModelsView>("list_models");
}

export function selectModel(id: string): Promise<void> {
  return invoke("select_model", { id });
}

export function downloadModel(id: string): Promise<void> {
  return invoke("download_model", { id });
}

export function cancelModelDownload(id: string): Promise<void> {
  return invoke("cancel_model_download", { id });
}

export function deleteModel(id: string): Promise<void> {
  return invoke("delete_model", { id });
}

// Event names match `PROGRESS_EVENT` and `CHANGED_EVENT` in models/commands.rs.
export function onModelProgress(handler: (progress: ModelProgress) => void): Promise<UnlistenFn> {
  return listen<ModelProgress>("model-progress", (event) => handler(event.payload));
}

export function onModelsChanged(handler: () => void): Promise<UnlistenFn> {
  return listen("models-changed", () => handler());
}
