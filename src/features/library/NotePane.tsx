import { useCallback, useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { AlertIcon, DownloadIcon, FinderIcon, MoreIcon, TrashIcon } from "./icons";
import {
  formatDuration,
  inputsLabel,
  noteBlocks,
  readNote,
  sourceLabel,
  startTitle,
  type NoteView,
  type Recording,
} from "./library";
import { Button } from "./ui";

// Rust rejects with plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

type NoteState = { note: NoteView } | { error: string } | null;

// Right side when a recording is selected: its note, or where it is on the way
// to one.
export function NotePane({
  recording,
  onShowInFinder,
  onMoveToTrash,
  onDownloadModels,
}: {
  recording: Recording;
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
  const closeMenu = useCallback(() => setMenu(false), []);
  const menuButton = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState<string | null>(null);
  const noteState = loaded.folder === folder ? loaded.state : null;

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

  const note = noteState && "note" in noteState ? noteState.note : null;
  const subtitle = [
    formatDuration(recording.duration_seconds),
    sourceLabel(note?.source ?? null),
    inputsLabel(note?.inputs ?? []),
  ]
    .filter(Boolean)
    .join(" · ");

  const run = (action: () => Promise<void>) => {
    setMenu(false);
    setError(null);
    action().catch((err) => setError(message(err)));
  };

  return (
    <div className="relative flex h-full flex-col">
      <header className="flex min-h-16 items-center gap-3 border-b border-line px-6 py-3">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[15px] font-semibold tabular-nums">
            {startTitle(recording.start)}
          </h1>
          {subtitle && <p className="truncate text-[12px] text-muted">{subtitle}</p>}
        </div>
        {status === "needs_models" && (
          <Button variant="primary" onClick={onDownloadModels}>
            <DownloadIcon className="size-3.5" />
            Download models
          </Button>
        )}
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
          {recording.status === "ready" ? (
            noteState && "error" in noteState ? (
              <Panel title="Anchovy can't read note.md" tone="failed">
                {noteState.error}
              </Panel>
            ) : (
              note && <NoteBody note={note} />
            )
          ) : (
            <StatePanel recording={recording} />
          )}
        </div>
      </div>
      {menu && (
        <MoreMenu
          trigger={menuButton}
          onClose={closeMenu}
          onShowInFinder={() => run(onShowInFinder)}
          onMoveToTrash={() => run(onMoveToTrash)}
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

function StatePanel({ recording }: { recording: Recording }) {
  switch (recording.status) {
    case "recording":
      return <Panel title="Recording">The audio is still being written.</Panel>;
    case "saved":
      return <Panel title="No note yet">The audio is saved. No note has been written.</Panel>;
    case "needs_models":
      return (
        <Panel title="The models for this note are not on this Mac" tone="attention">
          Download them in Models. The audio is saved.
        </Panel>
      );
    case "working":
      return (
        <Panel title="Writing the note">
          The audio is saved; the note appears here when it is ready.
        </Panel>
      );
    case "failed":
      return (
        <Panel title={recording.reason} tone="failed">
          The audio is saved and no note was written.
        </Panel>
      );
    case "ready":
      return null;
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
      <p className="mt-1 text-muted">{children}</p>
    </div>
  );
}

function MoreMenu({
  trigger,
  onClose,
  onShowInFinder,
  onMoveToTrash,
}: {
  trigger: RefObject<HTMLButtonElement | null>;
  onClose: () => void;
  onShowInFinder: () => void;
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
      <div className="my-1 border-t border-line" />
      <button type="button" role="menuitem" className={item} onClick={onMoveToTrash}>
        <TrashIcon className="text-muted" />
        Move to Trash
      </button>
    </div>
  );
}
