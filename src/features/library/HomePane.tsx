import { MicIcon, RecordDot, SpeakerIcon } from "./icons";
import { Button } from "./ui";

// What will be recorded. Plan step 4b fills these in from the system; until
// then the microphone is the default input and computer audio has not been
// asked for.
export type Sources = {
  microphone: string | null;
  computerAudio: "allowed" | "not_allowed" | null;
};

const unknownSources: Sources = { microphone: null, computerAudio: null };

// Always two rows: the chosen microphone, then computer audio. The second row
// says plainly when that side will not be recorded.
function SourceRows({ microphone, computerAudio }: Sources) {
  const notAllowed = computerAudio === "not_allowed";
  const computerAudioText = {
    allowed: "Will be recorded",
    not_allowed: "Not allowed",
    unknown: "Asked when you first record",
  }[computerAudio ?? "unknown"];
  return (
    <div className="divide-y divide-line rounded-lg border border-line text-left">
      <div className="flex h-12 items-center gap-3 px-4">
        <MicIcon className="shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] text-muted">Microphone</p>
          <p className="truncate font-medium">{microphone ?? "Default input"}</p>
        </div>
      </div>
      <div className="px-4 py-2">
        <div className="flex min-h-8 items-center gap-3">
          <SpeakerIcon className="shrink-0 text-muted" />
          <div className="min-w-0 flex-1">
            <p className="text-[12px] text-muted">Computer audio</p>
            <p className={`font-medium ${notAllowed ? "text-attention" : ""}`}>
              {computerAudioText}
            </p>
          </div>
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

// Right side when nothing is selected: the sources that will be used, and Record.
export function HomePane({ sources = unknownSources }: { sources?: Sources }) {
  return (
    <div className="flex h-full items-center justify-center px-8">
      <div className="w-[400px] text-center">
        <h1 className="text-[17px] font-semibold">Ready to record</h1>
        <p className="mt-1 text-muted">Anchovy records these sources into one file on this Mac.</p>
        <div className="mt-6">
          <SourceRows {...sources} />
        </div>
        {/* Recording arrives in plan step 4. */}
        <Button variant="primary" className="mt-6 h-9 px-5" disabled>
          <RecordDot />
          Record
        </Button>
      </div>
    </div>
  );
}
