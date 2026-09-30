import { convertFileSrc } from "@tauri-apps/api/core";
import { useRef, useState, type CSSProperties } from "react";
import { PauseIcon, PlayIcon } from "./icons";
import { clock, type RecordingAudio } from "./library";

// Plays audio.wav or audio.m4a from the recording folder, through Tauri's
// asset protocol, which only reaches the notes folder. Nothing is read until
// Play: `seconds` is the length the library read, shown until the file says.
export function Player({ audio, seconds }: { audio: RecordingAudio; seconds: number | null }) {
  const element = useRef<HTMLAudioElement>(null);
  const [playing, setPlaying] = useState(false);
  const [now, setNow] = useState(0);
  const [length, setLength] = useState<number | null>(seconds);
  const [failed, setFailed] = useState(false);

  function toggle() {
    const media = element.current;
    if (!media) return;
    if (playing) media.pause();
    else void media.play().catch(() => setFailed(true));
  }

  return (
    <div
      role="group"
      aria-label="Audio"
      className="flex h-11 items-center gap-3 rounded-lg border border-line px-2"
    >
      <audio
        ref={element}
        src={convertFileSrc(audio.path)}
        preload="none"
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onTimeUpdate={(event) => setNow(event.currentTarget.currentTime)}
        onLoadedMetadata={(event) => {
          const duration = event.currentTarget.duration;
          if (Number.isFinite(duration)) setLength(duration);
        }}
        onError={() => setFailed(true)}
      />
      <button
        type="button"
        aria-label={playing ? "Pause" : "Play"}
        disabled={failed}
        onClick={toggle}
        className="flex size-7 shrink-0 items-center justify-center rounded-full bg-hover enabled:hover:bg-line disabled:opacity-40"
      >
        {playing ? <PauseIcon className="size-3.5" /> : <PlayIcon className="size-3.5" />}
      </button>
      <span aria-label="Current time" className="text-[12px] text-muted tabular-nums">
        {clock(now)}
      </span>
      {failed ? (
        <span className="flex-1 text-[12px] text-muted">Anchovy can't play this file.</span>
      ) : (
        <input
          type="range"
          aria-label="Seek"
          min={0}
          max={length ?? 0}
          step="any"
          value={Math.min(now, length ?? 0)}
          onChange={(event) => {
            const at = Number(event.target.value);
            if (element.current) element.current.currentTime = at;
            setNow(at);
          }}
          className="seek h-1 min-w-0 flex-1"
          style={
            {
              "--progress": `${length ? (Math.min(now, length) / length) * 100 : 0}%`,
            } as CSSProperties
          }
        />
      )}
      <span aria-label="Length" className="text-[12px] text-muted tabular-nums">
        {length === null ? "" : clock(length, true)}
      </span>
      <span className="pr-2 text-[12px] text-faint">{audio.name}</span>
    </div>
  );
}
