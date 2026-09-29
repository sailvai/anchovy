import type { ModelView } from "../../ipc/models";
import type { Stage } from "../../ipc/notes";

// Sentences end with ". " or at the end; "1.7B" does not end one.
function splitReason(reason: string): [string, string] {
  const trimmed = reason.trim();
  const end = trimmed.indexOf(". ");
  if (end === -1) return [trimmed.replace(/\.$/, ""), ""];
  return [trimmed.slice(0, end), trimmed.slice(end + 2).trim()];
}

// A failed recording's reason: the first sentence is short enough for the
// list and is the panel's title; the rest says what to do.
export function headline(reason: string): string {
  return splitReason(reason)[0];
}

export function reasonDetail(reason: string): string {
  return splitReason(reason)[1];
}

// The list's detail next to Working.
export function stageLabel(stage: Stage): string {
  switch (stage.stage) {
    case "waiting":
      return "Waiting";
    case "transcribing":
      return "Transcribing";
    case "summarizing":
      return "Writing summary";
  }
}

// "27 of 42 min", or seconds for audio under a minute.
export function transcribedLabel(done: number, total: number): string {
  if (total < 60) return `${Math.floor(done)} of ${Math.round(total)} s`;
  return `${Math.floor(done / 60)} of ${Math.ceil(total / 60)} min`;
}

// The selected models that are not on this Mac yet.
export function missingModels(models: ModelView[]): ModelView[] {
  return models.filter((model) => model.selected && model.state.kind !== "ready");
}

export function missingTitle(missing: ModelView[]): string {
  if (missing.length === 1) {
    const kind = missing[0].role === "transcribe" ? "transcription" : "summary";
    return `The ${kind} model is not on this Mac`;
  }
  return "The models for this note are not on this Mac";
}
