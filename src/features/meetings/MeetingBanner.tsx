import type { MeetingPrompt } from "../../ipc/meetings";
import { MeetingIcon, RecordDot } from "../library/icons";
import { Button } from "../library/ui";

// The meeting prompt. It sits above the window content and never blocks it;
// the same question is also a macOS notification.
export function MeetingBanner({
  prompt,
  canRecord,
  onRecord,
  onNotNow,
}: {
  prompt: MeetingPrompt;
  canRecord: boolean;
  onRecord: () => void;
  onNotNow: () => void;
}) {
  return (
    <div
      role="status"
      aria-label="Meeting prompt"
      className="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-surface-raised px-4"
    >
      <MeetingIcon className="shrink-0 text-muted" />
      <p className="min-w-0 flex-1 truncate">
        <span className="font-medium">{prompt.headline}</span>{" "}
        <span className="text-muted">{prompt.body}</span>
      </p>
      <Button size="sm" variant="ghost" onClick={onNotNow}>
        Not now
      </Button>
      <Button size="sm" variant="primary" disabled={!canRecord} onClick={onRecord}>
        <RecordDot />
        Record
      </Button>
    </div>
  );
}
