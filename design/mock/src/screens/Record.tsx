import { liveRecording, microphoneName } from "../data";
import { MicIcon, RecordDot, SpeakerIcon, StopIcon } from "../icons";
import { Button, LevelMeter } from "../ui";

type ComputerAudio = "allowed" | "not-allowed";

// Always two rows: the chosen microphone, then computer audio. The second row
// says plainly when that side is not being recorded.
function Sources({ live, computerAudio }: { live: boolean; computerAudio: ComputerAudio }) {
  const allowed = computerAudio === "allowed";
  return (
    <div className="divide-y divide-line rounded-lg border border-line text-left">
      <div className="flex h-12 items-center gap-3 px-4">
        <MicIcon className="shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] text-muted">Microphone</p>
          <p className="truncate font-medium">{microphoneName}</p>
        </div>
        {live && <LevelMeter levels={[0.3, 0.55, 0.8, 0.6, 0.35, 0.7, 0.45, 0.2]} />}
      </div>
      <div className="px-4 py-2">
        <div className="flex min-h-8 items-center gap-3">
          <SpeakerIcon className="shrink-0 text-muted" />
          <div className="min-w-0 flex-1">
            <p className="text-[12px] text-muted">Computer audio</p>
            <p className={`font-medium ${allowed ? "" : "text-attention"}`}>
              {allowed ? (live ? "Recording" : "Will be recorded") : "Not allowed"}
            </p>
          </div>
          {live && allowed && <LevelMeter levels={[0.2, 0.4, 0.3, 0.65, 0.5, 0.3, 0.55, 0.4]} />}
          {!allowed && <Button size="sm">Allow</Button>}
        </div>
        {!allowed && (
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
export function Home({ computerAudio = "allowed" }: { computerAudio?: ComputerAudio }) {
  return (
    <div className="flex h-full items-center justify-center px-8">
      <div className="w-[400px] text-center">
        <h1 className="text-[17px] font-semibold">Ready to record</h1>
        <p className="mt-1 text-muted">Anchovy records these sources into one file on this Mac.</p>
        <div className="mt-6">
          <Sources live={false} computerAudio={computerAudio} />
        </div>
        <Button variant="primary" className="mt-6 h-9 px-5">
          <RecordDot />
          Record
        </Button>
      </div>
    </div>
  );
}

export function RecordingPane() {
  return (
    <div className="flex h-full items-center justify-center px-8">
      <div className="w-[400px] text-center">
        <p className="inline-flex items-center gap-2 text-[12px] font-medium text-recording">
          <RecordDot />
          Recording
        </p>
        <p className="mt-2 text-[44px] leading-none font-light tracking-[-0.02em] tabular-nums">
          00:12:48
        </p>
        <p className="mt-3 text-[12px] text-muted tabular-nums">
          73.7 MB · WAV, 48 kHz, 16-bit, mono
        </p>
        <p className="text-[12px] text-muted">
          Started {liveRecording.time} · {liveRecording.folder}/audio.wav
        </p>
        <div className="mt-6">
          <Sources live computerAudio="allowed" />
        </div>
        <Button variant="primary" className="mt-6 h-9 px-5">
          <StopIcon className="size-3.5" />
          Stop
        </Button>
      </div>
    </div>
  );
}
