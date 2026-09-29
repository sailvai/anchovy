import type { Access } from "../../ipc/setup";
import { MicIcon, SpeakerIcon } from "./icons";
import { Button } from "./ui";

// "checking" while the one-second computer audio check runs.
export type ComputerAudioRow = Access | "checking";

// Always two rows: the chosen microphone, then computer audio. The second row
// says plainly when that side is not being recorded.
export function SourceRows({
  microphone,
  computerAudio,
  live,
  onAllow,
}: {
  microphone: string | null;
  computerAudio: ComputerAudioRow;
  live: boolean;
  onAllow?: () => void;
}) {
  const allowed = computerAudio === "allowed";
  const notAllowed = computerAudio === "denied" || computerAudio === "not_asked";
  const text = allowed
    ? live
      ? "Recording"
      : "Will be recorded"
    : notAllowed
      ? "Not allowed"
      : "Checking…";
  return (
    <div className="divide-y divide-line rounded-lg border border-line text-left">
      <div className="flex h-12 items-center gap-3 px-4">
        <MicIcon className="shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] text-muted">Microphone</p>
          <p className="truncate font-medium">{microphone ?? "No microphone found"}</p>
        </div>
      </div>
      <div className="px-4 py-2">
        <div className="flex min-h-8 items-center gap-3">
          <SpeakerIcon className="shrink-0 text-muted" />
          <div className="min-w-0 flex-1">
            <p className="text-[12px] text-muted">Computer audio</p>
            <p className={`font-medium ${notAllowed ? "text-attention" : ""}`}>{text}</p>
          </div>
          {notAllowed && onAllow && (
            <Button size="sm" onClick={onAllow}>
              Allow
            </Button>
          )}
        </div>
        {notAllowed && (
          <p className="mt-1 mb-1 ml-7 text-[12px] text-muted">
            Only the microphone is recorded. The other side of online meetings will not be in the
            recording.
          </p>
        )}
      </div>
    </div>
  );
}
