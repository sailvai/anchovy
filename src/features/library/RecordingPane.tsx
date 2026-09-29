import { RecordDot, StopIcon } from "./icons";
import { formatElapsed, formatFileSize, timeOfDay, startOfFolder } from "./library";
import { SourceRows, type ComputerAudioRow } from "./Sources";
import { Button } from "./ui";

// Right side while recording: elapsed time and file size from the
// recording-progress event, the two sources, and Stop.
export function RecordingPane({
  folder,
  microphone,
  computerAudio,
  seconds,
  bytes,
  stopping,
  error,
  onStop,
}: {
  folder: string;
  microphone: string;
  computerAudio: ComputerAudioRow;
  seconds: number;
  bytes: number;
  stopping: boolean;
  error: string | null;
  onStop: () => void;
}) {
  const start = startOfFolder(folder);
  return (
    <div className="flex h-full items-center justify-center px-8">
      <div className="w-[400px] text-center">
        <p className="inline-flex items-center gap-2 text-[12px] font-medium text-recording">
          <RecordDot />
          Recording
        </p>
        <p
          className="mt-2 text-[44px] leading-none font-light tracking-[-0.02em] tabular-nums"
          aria-label="Elapsed time"
        >
          {formatElapsed(seconds)}
        </p>
        <p className="mt-3 text-[12px] text-muted tabular-nums">
          {formatFileSize(bytes)} · WAV, 48 kHz, 16-bit, mono
        </p>
        <p className="text-[12px] text-muted">
          {start ? `Started ${timeOfDay(start)} · ` : ""}
          {folder}/audio.wav
        </p>
        <div className="mt-6">
          <SourceRows microphone={microphone} computerAudio={computerAudio} live />
        </div>
        <Button variant="primary" className="mt-6 h-9 px-5" disabled={stopping} onClick={onStop}>
          <StopIcon className="size-3.5" />
          Stop
        </Button>
        {error && (
          <p role="alert" className="mt-3 text-[12px] text-failed">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
