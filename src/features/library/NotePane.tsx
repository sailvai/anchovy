import { useCallback, useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { listModels, onModelsChanged, type ModelView } from "../../ipc/models";
import type { Stage } from "../../ipc/notes";
import { getSettings } from "../../ipc/settings";
import {
  AlertIcon,
  CheckIcon,
  DownloadIcon,
  FinderIcon,
  MoreIcon,
  RefreshIcon,
  Spinner,
  TrashIcon,
} from "./icons";
import {
  formatDuration,
  inputsLabel,
  noteBlocks,
  readNote,
  recordingAudio,
  sourceLabel,
  startTitle,
  type NoteView,
  type Recording,
  type RecordingAudio,
} from "./library";
import { Player } from "./Player";
import { headline, missingModels, missingTitle, reasonDetail, transcribedLabel } from "./notes";
import { Button, Progress } from "./ui";

// Rust rejects with plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

type NoteState = { note: NoteView } | { error: string } | null;

// Right side when a recording is selected: its note, or where it is on the way
// to one. The primary button follows the state: Generate note, Download
// models, or Retry. Regenerate note is in the more-actions menu and asks
// first, because the user may have edited note.md.
export function NotePane({
  recording,
  stage,
  onGenerate,
  onShowInFinder,
  onMoveToTrash,
  onDownloadModels,
}: {
  recording: Recording;
  // Where the note is while Working, when the pipeline has said.
  stage: Stage | null;
  // `replace` is the answer to "Replace note.md?".
  onGenerate: (replace: boolean) => Promise<void>;
  onShowInFinder: () => Promise<void>;
  onMoveToTrash: () => Promise<void>;
  onDownloadModels: () => void;
}) {
  const { folder, status } = recording;
  const [loaded, setLoaded] = useState<{ folder: string; state: NoteState }>({
    folder,
    state: null,
  });
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const closeMenu = useCallback(() => setMenu(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [models, setModels] = useState<ModelView[] | null>(null);
  const [automatic, setAutomatic] = useState<boolean | null>(null);
  const noteState = loaded.folder === folder ? loaded.state : null;
  const [audio, setAudio] = useState<{
    folder: string;
    audio: RecordingAudio | null;
    // Why the audio cannot be played, when Rust says so.
    error?: string;
  }>({ folder, audio: null });
  const beingRecorded = status === "recording";

  // The player, for any recording with audio, except while it is recorded.
  useEffect(() => {
    if (beingRecorded) return;
    let current = true;
    recordingAudio(folder).then(
      (found) => current && setAudio({ folder, audio: found }),
      (err) => current && setAudio({ folder, audio: null, error: message(err) }),
    );
    return () => {
      current = false;
    };
  }, [folder, beingRecorded]);
  const playable = !beingRecorded && audio.folder === folder ? audio.audio : null;
  const unplayable = !beingRecorded && audio.folder === folder ? audio.error : undefined;

  useEffect(() => {
    if (status !== "ready") return;
    let current = true;
    readNote(folder).then(
      (note) => current && setLoaded({ folder, state: { note } }),
      (err) => current && setLoaded({ folder, state: { error: message(err) } }),
    );
    return () => {
      current = false;
    };
  }, [folder, status]);

  // Which models are here decides Download models, and Regenerate names them.
  const wantsModels = status === "needs_models" || status === "failed" || status === "ready";
  useEffect(() => {
    if (!wantsModels) return;
    let current = true;
    const load = () =>
      listModels().then(
        (view) => current && setModels(view?.models ?? []),
        () => current && setModels([]),
      );
    void load();
    const unlisten = onModelsChanged(() => void load());
    return () => {
      current = false;
      void unlisten.then((fn) => fn());
    };
  }, [wantsModels]);

  useEffect(() => {
    if (status !== "saved") return;
    let current = true;
    getSettings().then(
      (settings) => current && setAutomatic(settings?.generate_notes_automatically ?? null),
      () => {},
    );
    return () => {
      current = false;
    };
  }, [status]);

  const note = noteState && "note" in noteState ? noteState.note : null;
  const subtitle = [
    formatDuration(recording.duration_seconds),
    sourceLabel(note?.source ?? null),
    inputsLabel(note?.inputs ?? []),
  ]
    .filter(Boolean)
    .join(" · ");
  const missing = models ? missingModels(models) : [];
  const selected = (role: ModelView["role"]) =>
    models?.find((model) => model.selected && model.role === role)?.display_name;

  const run = (action: () => Promise<void>) => {
    setMenu(false);
    setConfirm(false);
    setError(null);
    action().catch((err) => setError(message(err)));
  };

  let primary: ReactNode = null;
  if (status === "saved") {
    primary = (
      <Button variant="primary" onClick={() => run(() => onGenerate(false))}>
        Generate note
      </Button>
    );
  } else if (status === "needs_models" || (status === "failed" && missing.length > 0)) {
    primary = (
      <Button variant="primary" onClick={onDownloadModels}>
        <DownloadIcon className="size-3.5" />
        Download models
      </Button>
    );
  } else if (status === "failed") {
    // A failed Regenerate note already had the user's answer.
    primary = (
      <Button variant="primary" onClick={() => run(() => onGenerate(true))}>
        <RefreshIcon className="size-3.5" />
        Retry
      </Button>
    );
  }

  return (
    <div className="relative flex h-full flex-col">
      <header className="flex min-h-16 items-center gap-3 border-b border-line px-6 py-3">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[15px] font-semibold tabular-nums">
            {startTitle(recording.start)}
          </h1>
          {subtitle && <p className="truncate text-[12px] text-muted">{subtitle}</p>}
        </div>
        {primary}
        <Button
          ref={menuButton}
          variant="ghost"
          aria-label="More actions"
          aria-haspopup="menu"
          aria-expanded={menu}
          size="icon"
          className={menu ? "bg-hover text-text" : ""}
          onClick={() => setMenu(!menu)}
        >
          <MoreIcon />
        </Button>
      </header>
      {error && (
        <p role="alert" className="border-b border-line px-6 py-2 text-[12px] text-failed">
          {error}
        </p>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        <div className="max-w-[620px]">
          {unplayable && (
            <p className="mb-6 flex h-11 items-center rounded-lg border border-line px-4 text-[12px] text-muted">
              {unplayable}
            </p>
          )}
          {playable && (
            <div className="mb-6">
              <Player key={playable.path} audio={playable} seconds={recording.duration_seconds} />
            </div>
          )}
          {status === "ready" ? (
            noteState && "error" in noteState ? (
              <Panel title="Anchovy can't read note.md" tone="failed">
                {noteState.error}
              </Panel>
            ) : (
              note && <NoteBody note={note} />
            )
          ) : (
            <StatePanel
              recording={recording}
              stage={stage}
              automatic={automatic}
              models={models}
              missing={missing}
            />
          )}
        </div>
      </div>
      {menu && (
        <MoreMenu
          trigger={menuButton}
          canRegenerate={status === "ready"}
          onClose={closeMenu}
          onShowInFinder={() => run(onShowInFinder)}
          onRegenerate={() => {
            setMenu(false);
            setConfirm(true);
          }}
          onMoveToTrash={() => run(onMoveToTrash)}
        />
      )}
      {confirm && (
        <RegenerateDialog
          transcription={selected("transcribe")}
          summary={selected("summarize")}
          onCancel={() => setConfirm(false)}
          onReplace={() => run(() => onGenerate(true))}
        />
      )}
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

// The same text as note.md. The Audio section is only a link to the file,
// which Show in Finder already reaches.
function NoteBody({ note }: { note: NoteView }) {
  return (
    <div className="select-text">
      {note.sections
        .filter(({ heading }) => heading !== "Audio")
        .map(({ heading, body }, index) => (
          <Section key={index} title={heading}>
            <div className="space-y-2">
              {noteBlocks(body).map((block, blockIndex) => (
                <Block key={blockIndex} block={block} />
              ))}
            </div>
          </Section>
        ))}
    </div>
  );
}

function Block({ block }: { block: ReturnType<typeof noteBlocks>[number] }) {
  if (block.kind === "bullets") {
    if (block.items.length === 0) return <p className="text-muted">None</p>;
    return (
      <ul className="space-y-1">
        {block.items.map((item, index) => (
          <li key={index} className="flex gap-2">
            <span className="text-faint">–</span>
            <span>{item}</span>
          </li>
        ))}
      </ul>
    );
  }
  if (block.kind === "transcript") {
    return (
      <div className="space-y-2">
        {block.lines.map(({ time, text }, index) => (
          <p key={index} className="flex gap-3">
            <span className="w-16 shrink-0 text-[12px] leading-[1.6rem] text-faint tabular-nums">
              {time}
            </span>
            <span>{text}</span>
          </p>
        ))}
      </div>
    );
  }
  return <p className="whitespace-pre-line">{block.text}</p>;
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

const whenReady = "The audio is saved; the note appears here when it is ready.";

function WorkingPanel({ stage }: { stage: Stage | null }) {
  if (stage?.stage === "summarizing") {
    return (
      <Panel title="Writing the note">
        <p>Step 2 of 2. {whenReady}</p>
        <ul className="mt-3">
          <Step state="done" label="Transcribing" />
          <Step
            state="active"
            label="Writing summary"
            detail={stage.total > 1 ? `Part ${stage.done + 1} of ${stage.total}` : undefined}
          />
        </ul>
      </Panel>
    );
  }
  if (stage?.stage === "transcribing") {
    const { done_seconds: done, total_seconds: total } = stage;
    return (
      <Panel title="Writing the note">
        <p>Step 1 of 2. {whenReady}</p>
        <ul className="mt-3">
          <Step state="active" label="Transcribing" detail={transcribedLabel(done, total)} />
          <li className="mb-2 ml-7">
            <Progress value={total > 0 ? (done / total) * 100 : 0} />
          </li>
          <Step state="waiting" label="Writing summary" />
        </ul>
      </Panel>
    );
  }
  return (
    <Panel title="Writing the note">
      <p>
        {stage?.stage === "waiting" ? "Waiting for another note to finish. " : ""}
        {whenReady}
      </p>
      <ul className="mt-3">
        <Step state="waiting" label="Transcribing" />
        <Step state="waiting" label="Writing summary" />
      </ul>
    </Panel>
  );
}

function StatePanel({
  recording,
  stage,
  automatic,
  models,
  missing,
}: {
  recording: Recording;
  stage: Stage | null;
  automatic: boolean | null;
  models: ModelView[] | null;
  missing: ModelView[];
}) {
  switch (recording.status) {
    case "recording":
      return <Panel title="Recording">The audio is still being written.</Panel>;
    case "saved":
      return (
        <Panel title="No note yet">
          {automatic === false
            ? "Automatic notes are off. Choose Generate note to write one on this Mac."
            : "The audio is saved. Choose Generate note to write one on this Mac."}
        </Panel>
      );
    case "needs_models": {
      const selected = models?.filter((model) => model.selected) ?? [];
      return (
        <Panel title={missingTitle(missing)} tone="attention">
          <p>Anchovy starts the note as soon as the download finishes.</p>
          {selected.length > 0 && (
            <ul className="mt-3">
              {selected.map((model) => (
                <Step
                  key={model.id}
                  state={model.state.kind === "ready" ? "done" : "waiting"}
                  label={model.display_name}
                  detail={modelDetail(model)}
                />
              ))}
            </ul>
          )}
        </Panel>
      );
    }
    case "working":
      return <WorkingPanel stage={stage} />;
    case "failed": {
      const detail = reasonDetail(recording.reason);
      return (
        <Panel title={headline(recording.reason)} tone="failed">
          {detail ? `${detail} ` : ""}The audio is saved and no note was written.
        </Panel>
      );
    }
    case "ready":
      return null;
  }
}

function modelDetail(model: ModelView): string {
  const gigabytes = (bytes: number) => `${(bytes / 1e9).toFixed(1)} GB`;
  switch (model.state.kind) {
    case "ready":
      return "On this Mac";
    case "downloading":
      return `Downloading ${gigabytes(model.state.downloaded)} of ${gigabytes(model.size_bytes)}`;
    default:
      return `${gigabytes(model.size_bytes - ("downloaded" in model.state ? model.state.downloaded : 0))} to download`;
  }
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

function MoreMenu({
  trigger,
  canRegenerate,
  onClose,
  onShowInFinder,
  onRegenerate,
  onMoveToTrash,
}: {
  trigger: RefObject<HTMLButtonElement | null>;
  canRegenerate: boolean;
  onClose: () => void;
  onShowInFinder: () => void;
  onRegenerate: () => void;
  onMoveToTrash: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    ref.current?.querySelector("button")?.focus();
    const outside = (event: MouseEvent) => {
      const target = event.target as Node;
      // The trigger toggles the menu itself.
      if (!ref.current?.contains(target) && !trigger.current?.contains(target)) onClose();
    };
    document.addEventListener("mousedown", outside);
    return () => document.removeEventListener("mousedown", outside);
  }, [trigger, onClose]);

  const item =
    "flex h-8 w-full items-center gap-2.5 rounded px-2.5 text-left hover:bg-hover focus:bg-hover focus:outline-none";
  return (
    <div
      ref={ref}
      role="menu"
      aria-label="More actions"
      onKeyDown={(event) => event.key === "Escape" && onClose()}
      className="absolute top-[52px] right-6 z-10 w-52 rounded-lg border border-line bg-surface p-1 shadow-[0_6px_20px_rgba(0,0,0,0.08)]"
    >
      <button type="button" role="menuitem" className={item} onClick={onShowInFinder}>
        <FinderIcon className="text-muted" />
        Show in Finder
      </button>
      {canRegenerate && (
        <button type="button" role="menuitem" className={item} onClick={onRegenerate}>
          <RefreshIcon className="text-muted" />
          Regenerate note…
        </button>
      )}
      <div className="my-1 border-t border-line" />
      <button type="button" role="menuitem" className={item} onClick={onMoveToTrash}>
        <TrashIcon className="text-muted" />
        Move to Trash
      </button>
    </div>
  );
}

// Overwriting a note the user may have edited asks first. The window stays
// usable; only this pane is covered.
function RegenerateDialog({
  transcription,
  summary,
  onCancel,
  onReplace,
}: {
  transcription: string | undefined;
  summary: string | undefined;
  onCancel: () => void;
  onReplace: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => cancel.current?.focus(), []);
  const models =
    transcription && summary ? `with ${transcription} and ${summary}` : "with the selected models";
  return (
    <div
      className="absolute inset-0 z-20 flex items-center justify-center bg-black/25"
      onKeyDown={(event) => event.key === "Escape" && onCancel()}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="regenerate-title"
        className="w-[400px] rounded-xl border border-line bg-surface p-5 shadow-[0_12px_32px_rgba(0,0,0,0.16)]"
      >
        <h2 id="regenerate-title" className="text-[15px] font-semibold">
          Replace note.md?
        </h2>
        <p className="mt-2 text-muted">
          Anchovy writes a new note {models}. Changes you made to this note, for example in
          Obsidian, will be lost. The audio stays.
        </p>
        <div className="mt-5 flex justify-end gap-2">
          <Button ref={cancel} onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="primary" onClick={onReplace}>
            Replace Note
          </Button>
        </div>
      </div>
    </div>
  );
}
