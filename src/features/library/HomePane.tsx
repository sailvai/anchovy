import { RecordDot } from "./icons";
import { SourceRows, type ComputerAudioRow } from "./Sources";
import { Button } from "./ui";

// Right side when nothing is selected: the sources that will be used, and
// Record. Record is off until there is a notes folder and the microphone is
// allowed; `blocked` says which is missing.
export function HomePane({
  microphone,
  computerAudio,
  canRecord,
  blocked,
  error,
  onRecord,
  onAllowComputerAudio,
}: {
  microphone: string | null;
  computerAudio: ComputerAudioRow;
  canRecord: boolean;
  blocked: string | null;
  error: string | null;
  onRecord: () => void;
  onAllowComputerAudio: () => void;
}) {
  return (
    <div className="flex h-full items-center justify-center px-8">
      <div className="w-[400px] text-center">
        <h1 className="text-[17px] font-semibold">Ready to record</h1>
        <p className="mt-1 text-muted">Anchovy records these sources into one file on this Mac.</p>
        <div className="mt-6">
          <SourceRows
            microphone={microphone}
            computerAudio={computerAudio}
            live={false}
            onAllow={onAllowComputerAudio}
          />
        </div>
        <Button
          variant="primary"
          className="mt-6 h-9 px-5"
          disabled={!canRecord}
          onClick={onRecord}
        >
          <RecordDot />
          Record
        </Button>
        {blocked && <p className="mt-3 text-[12px] text-muted">{blocked}</p>}
        {error && (
          <p role="alert" className="mt-3 text-[12px] text-failed">
            {error}
          </p>
        )}
      </div>
    </div>
  );
}
