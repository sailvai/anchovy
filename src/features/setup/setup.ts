import type { ModelView, ModelsView } from "../../ipc/models";
import type { NotesFolder, SetupStatus } from "../../ipc/setup";

// The models first launch downloads: the preselected one for each role,
// transcription first.
export function defaultModels(view: ModelsView): ModelView[] {
  const order = ["transcribe", "summarize"];
  return view.models
    .filter((model) => model.selected)
    .sort((a, b) => order.indexOf(a.role) - order.indexOf(b.role));
}

export function formatGb(bytes: number): string {
  return `${(bytes / 1e9).toFixed(1)} GB`;
}

// Bytes on disk so far, from the list and any newer progress events.
export function downloadedBytes(model: ModelView, progress: Record<string, number>): number {
  if (model.state.kind === "ready") return model.size_bytes;
  const listed = "downloaded" in model.state ? model.state.downloaded : 0;
  return Math.min(model.size_bytes, Math.max(listed, progress[model.id] ?? 0));
}

export function totalProgress(models: ModelView[], progress: Record<string, number>) {
  return {
    downloaded: models.reduce((sum, model) => sum + downloadedBytes(model, progress), 0),
    total: models.reduce((sum, model) => sum + model.size_bytes, 0),
  };
}

export function timeLeft(bytesLeft: number, bytesPerSecond: number): string | null {
  if (!(bytesPerSecond > 0)) return null;
  const minutes = Math.ceil(bytesLeft / bytesPerSecond / 60);
  return minutes <= 1 ? "Less than a minute left" : `About ${minutes} minutes left`;
}

export function folderNote(folder: NotesFolder): string {
  if (!folder.exists) return "New folder. Anchovy creates it when you continue.";
  const kind = folder.obsidian_vault ? "Obsidian vault" : "Existing folder";
  return `${kind}. Each recording gets its own folder inside it.`;
}

// Downloads one model at a time. `start` asks Rust to begin; `finished`
// resolves when that download stops. A model that fails to start is left for
// the Models screen, and the next one still downloads.
export async function downloadInOrder(
  ids: string[],
  steps: { start: (id: string) => Promise<void>; finished: (id: string) => Promise<void> },
): Promise<void> {
  for (const id of ids) {
    try {
      await steps.start(id);
    } catch {
      continue;
    }
    await steps.finished(id);
  }
}

// The first step still to do: a folder, then the microphone, then models.
export function firstStep(status: SetupStatus): 1 | 2 | 3 {
  if (!status.notes_folder) return 1;
  if (status.microphone === "not_asked") return 2;
  return 3;
}
