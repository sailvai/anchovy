import type { ReactNode } from "react";
import { note, summaryModels, transcriptionModels } from "../data";
import {
  AlertIcon,
  CheckIcon,
  DownloadIcon,
  FinderIcon,
  MoreIcon,
  PlayIcon,
  RefreshIcon,
  Spinner,
  TrashIcon,
} from "../icons";
import { Button, Progress } from "../ui";
import { PaneHeader } from "./Window";

export type NoteState = "saved" | "needs-models" | "working" | "failed" | "ready";

type Meta = { heading: string; duration: string; length: string; source: string; inputs: string };

const metaFor: Record<NoteState, Meta> = {
  saved: {
    heading: "2026-09-25 10:00",
    duration: "9 min",
    length: "09:12",
    source: "Manual",
    inputs: "Microphone only",
  },
  "needs-models": {
    heading: "2026-09-26 11:30",
    duration: "18 min",
    length: "18:03",
    source: "Meeting",
    inputs: "Microphone, computer audio",
  },
  working: {
    heading: "2026-09-26 14:10",
    duration: "42 min",
    length: "42:00",
    source: "Meeting",
    inputs: "Microphone, computer audio",
  },
  failed: {
    heading: "2026-09-26 09:05",
    duration: "1 h 06 min",
    length: "1:06:21",
    source: "Manual",
    inputs: "Microphone, computer audio",
  },
  ready: {
    heading: note.heading,
    duration: note.duration,
    length: "27:14",
    source: note.source,
    inputs: note.inputs,
  },
};

// The primary button changes with the state; everything else is in the menu.
const primaryFor: Record<NoteState, ReactNode> = {
  saved: <Button variant="primary">Generate note</Button>,
  "needs-models": (
    <Button variant="primary">
      <DownloadIcon className="size-3.5" />
      Download models
    </Button>
  ),
  working: null,
  failed: (
    <Button variant="primary">
      <RefreshIcon className="size-3.5" />
      Retry
    </Button>
  ),
  ready: null,
};

export function NotePane({
  state,
  menu = false,
  confirm = false,
}: {
  state: NoteState;
  menu?: boolean;
  confirm?: boolean;
}) {
  const meta = metaFor[state];
  return (
    <div className="flex h-full flex-col">
      <PaneHeader
        title={meta.heading}
        subtitle={`${meta.duration} · ${meta.source} · ${meta.inputs}`}
        actions={
          <>
            {primaryFor[state]}
            <Button
              variant="ghost"
              aria-label="More actions"
              size="icon"
              className={menu ? "bg-hover text-text" : ""}
            >
              <MoreIcon />
            </Button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-hidden px-6 py-5">
        <div className="max-w-[620px]">
          <Player length={meta.length} audio={state === "saved" ? "audio.m4a" : note.audio} />
          <div className="mt-6">
            {state === "ready" ? <NoteBody /> : <StatePanel state={state} />}
          </div>
        </div>
      </div>
      {menu && <MoreMenu />}
      {confirm && <RegenerateDialog />}
    </div>
  );
}

function Player({ length, audio }: { length: string; audio: string }) {
  return (
    <div className="flex h-11 items-center gap-3 rounded-lg border border-line px-2">
      <button
        type="button"
        aria-label="Play"
        className="flex size-7 shrink-0 items-center justify-center rounded-full bg-hover"
      >
        <PlayIcon className="size-3.5" />
      </button>
      <span className="text-[12px] text-muted tabular-nums">0:00</span>
      <span className="relative h-1 flex-1 rounded-full bg-line">
        <span className="absolute top-1/2 left-0 size-2.5 -translate-y-1/2 rounded-full border border-line-strong bg-surface" />
      </span>
      <span className="text-[12px] text-muted tabular-nums">{length}</span>
      <span className="pr-2 text-[12px] text-faint">{audio}</span>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="mt-6 first:mt-0">
      <h2 className="text-[14px] font-semibold">{title}</h2>
      <div className="mt-2 text-[13.5px] leading-relaxed">{children}</div>
    </section>
  );
}

function Bullets({ items }: { items: string[] }) {
  return (
    <ul className="space-y-1">
      {items.map((item) => (
        <li key={item} className="flex gap-2">
          <span className="text-faint">–</span>
          <span>{item}</span>
        </li>
      ))}
    </ul>
  );
}

// Same text as note.md: Summary, Decisions, Action items, Transcript.
function NoteBody() {
  return (
    <div className="select-text">
      <Section title="Summary">
        <p>{note.summary}</p>
      </Section>
      <Section title="Decisions">
        <Bullets items={note.decisions} />
      </Section>
      <Section title="Action items">
        <Bullets items={note.actionItems} />
      </Section>
      <Section title="Transcript">
        <div className="space-y-2">
          {note.transcript.map(([time, text]) => (
            <p key={time} className="flex gap-3">
              <span className="w-16 shrink-0 text-[12px] leading-[1.6rem] text-faint tabular-nums">
                {time}
              </span>
              <span>{text}</span>
            </p>
          ))}
        </div>
      </Section>
    </div>
  );
}

function Step({
  state,
  label,
  detail,
}: {
  state: "done" | "active" | "waiting";
  label: string;
  detail?: string;
}) {
  return (
    <li className="flex items-center gap-3 py-1.5">
      <span className="flex w-4 justify-center">
        {state === "done" && <CheckIcon className="size-3.5 text-ready" />}
        {state === "active" && <Spinner className="text-accent" />}
        {state === "waiting" && <span className="size-1.5 rounded-full bg-line-strong" />}
      </span>
      <span className={`flex-1 ${state === "waiting" ? "text-muted" : ""}`}>{label}</span>
      {detail && <span className="text-[12px] text-muted tabular-nums">{detail}</span>}
    </li>
  );
}

function StatePanel({ state }: { state: Exclude<NoteState, "ready"> }) {
  if (state === "saved") {
    return (
      <Panel title="No note yet">
        Automatic notes are off. Choose Generate note to write one on this Mac.
      </Panel>
    );
  }
  if (state === "needs-models") {
    return (
      <Panel title="The summary model is not on this Mac" tone="attention">
        <p>Anchovy starts the note as soon as the download finishes.</p>
        <ul className="mt-3">
          <Step state="done" label={transcriptionModels[0].name} detail="On this Mac" />
          <Step
            state="waiting"
            label={summaryModels[0].name}
            detail={`${summaryModels[0].size} to download`}
          />
        </ul>
      </Panel>
    );
  }
  if (state === "working") {
    return (
      <Panel title="Writing the note">
        <p>Step 1 of 2. The audio is saved; the note appears here when it is ready.</p>
        <ul className="mt-3">
          <Step state="active" label="Transcribing" detail="27 of 42 min" />
          <li className="mb-2 ml-7">
            <Progress value={64} />
          </li>
          <Step state="waiting" label="Writing summary" />
        </ul>
      </Panel>
    );
  }
  return (
    <Panel title="Not enough memory to write the summary" tone="failed">
      The summary model needs about 3 GB of free memory. Close other apps, then choose Retry. The
      audio is saved and no note was written.
    </Panel>
  );
}

function Panel({
  title,
  tone,
  children,
}: {
  title: string;
  tone?: "attention" | "failed";
  children: ReactNode;
}) {
  const tones = {
    attention: "border-attention/30 bg-attention-soft",
    failed: "border-failed/30 bg-failed-soft",
  };
  return (
    <div className={`rounded-lg border px-4 py-3 ${tone ? tones[tone] : "border-line"}`}>
      <p className="flex items-center gap-2 font-medium">
        {tone === "failed" && <AlertIcon className="size-4 text-failed" />}
        {tone === "attention" && <DownloadIcon className="size-4 text-attention" />}
        {title}
      </p>
      <div className="mt-1 text-muted">{children}</div>
    </div>
  );
}

function MoreMenu() {
  const item = "flex h-8 items-center gap-2.5 rounded px-2.5";
  return (
    <div
      role="menu"
      className="absolute top-[52px] right-6 w-52 rounded-lg border border-line bg-surface p-1 shadow-[0_6px_20px_rgba(0,0,0,0.08)]"
    >
      <div role="menuitem" className={`${item} bg-hover`}>
        <FinderIcon className="text-muted" />
        Show in Finder
      </div>
      <div role="menuitem" className={item}>
        <RefreshIcon className="text-muted" />
        Regenerate note…
      </div>
      <div className="my-1 border-t border-line" />
      <div role="menuitem" className={item}>
        <TrashIcon className="text-muted" />
        Move to Trash
      </div>
    </div>
  );
}

// Overwriting a note the user may have edited asks first.
function RegenerateDialog() {
  return (
    <div className="absolute inset-0 flex items-center justify-center bg-black/25">
      <div
        role="alertdialog"
        aria-labelledby="regenerate-title"
        className="w-[400px] rounded-xl border border-line bg-surface p-5 shadow-[0_12px_32px_rgba(0,0,0,0.16)]"
      >
        <h2 id="regenerate-title" className="text-[15px] font-semibold">
          Replace note.md?
        </h2>
        <p className="mt-2 text-muted">
          Anchovy writes a new note with {transcriptionModels[0].name} and {summaryModels[0].name}.
          Changes you made to this note, for example in Obsidian, will be lost. The audio stays.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button>Cancel</Button>
          <Button variant="primary">Replace Note</Button>
        </div>
      </div>
    </div>
  );
}
