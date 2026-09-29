import { useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  downloadModel,
  listModels,
  onModelProgress,
  onModelsChanged,
  type ModelView,
} from "../../ipc/models";
import {
  checkComputerAudio,
  chooseDefaultNotesFolder,
  chooseNotesFolder,
  finishSetup,
  openPrivacySettings,
  requestMicrophone,
  type Access,
  type PrivacyPane,
  type SetupStatus,
} from "../../ipc/setup";
import { CheckIcon, FolderIcon, MicIcon, ModelsIcon, SpeakerIcon } from "../library/icons";
import { Button, Progress } from "../library/ui";
import {
  defaultModels,
  downloadInOrder,
  downloadedBytes,
  firstStep,
  folderNote,
  formatGb,
  timeLeft,
  totalProgress,
} from "./setup";
import { useOnWindowFocus } from "./useOnWindowFocus";

// Rust rejects plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

// First launch: three steps, one screen each, one primary button per screen.
function Step({
  step,
  title,
  body,
  children,
  actions,
}: {
  step: number;
  title: string;
  body: ReactNode;
  children: ReactNode;
  actions: ReactNode;
}) {
  return (
    <div className="flex h-full flex-col bg-surface">
      <div className="flex flex-1 items-center justify-center px-8">
        <div className="w-[460px]">
          <p className="text-[12px] font-medium text-muted">Anchovy setup · Step {step} of 3</p>
          <h1 className="mt-2 text-[22px] font-semibold tracking-[-0.01em]">{title}</h1>
          <p className="mt-2 text-[13px] text-muted">{body}</p>
          <div className="mt-6">{children}</div>
        </div>
      </div>
      <div className="flex items-center justify-between border-t border-line px-6 py-4">
        <StepDots current={step} />
        <div className="flex gap-2">{actions}</div>
      </div>
    </div>
  );
}

function StepDots({ current }: { current: number }) {
  return (
    <div className="flex gap-1.5" aria-label={`Step ${current} of 3`}>
      {[1, 2, 3].map((step) => (
        <span
          key={step}
          className={`h-1 w-6 rounded-full ${step <= current ? "bg-text" : "bg-line-strong"}`}
        />
      ))}
    </div>
  );
}

function Row({
  icon,
  title,
  body,
  aside,
  children,
}: {
  icon: ReactNode;
  title: string;
  body: ReactNode;
  aside?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="px-4 py-3">
      <div className="flex items-start gap-3">
        <span className="mt-0.5 text-muted">{icon}</span>
        <div className="min-w-0 flex-1">
          <p className="truncate font-medium">{title}</p>
          <p className="text-[12px] text-muted">{body}</p>
        </div>
        {aside}
      </div>
      {children}
    </div>
  );
}

function Box({ children }: { children: ReactNode }) {
  return <div className="divide-y divide-line rounded-lg border border-line">{children}</div>;
}

function ErrorLine({ error }: { error: string | null }) {
  if (!error) return null;
  return (
    <p role="alert" className="mt-3 text-[12px] text-failed">
      {error}
    </p>
  );
}

function Allowed() {
  return (
    <span className="inline-flex h-7 items-center gap-1 text-[12px] font-medium text-ready">
      <CheckIcon className="size-3.5" />
      Allowed
    </span>
  );
}

function NotAllowed() {
  return (
    <span className="inline-flex h-7 items-center text-[12px] font-medium text-attention">
      Not allowed
    </span>
  );
}

function Checking() {
  return <span className="inline-flex h-7 items-center text-[12px] text-muted">Checking…</span>;
}

function DeniedNote({ pane, children }: { pane: PrivacyPane; children: ReactNode }) {
  return (
    <div className="mt-3 ml-7 rounded-md bg-attention-soft px-3 py-2.5 text-[12px]">
      <p>{children}</p>
      <Button size="sm" className="mt-2" onClick={() => void openPrivacySettings(pane)}>
        Open System Settings
      </Button>
    </div>
  );
}

function FolderStep({
  status,
  onChosen,
  onNext,
}: {
  status: SetupStatus;
  onChosen: (next: SetupStatus) => void;
  onNext: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const folder = status.notes_folder ?? status.default_folder;

  async function run(pick: () => Promise<SetupStatus["notes_folder"]>, advance: boolean) {
    setBusy(true);
    setError(null);
    try {
      const chosen = await pick();
      if (!chosen) return;
      onChosen({ ...status, notes_folder: chosen });
      if (advance) onNext();
    } catch (err) {
      setError(`Anchovy can't use this folder. ${message(err)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Step
      step={1}
      title="Choose a notes folder"
      body="Anchovy saves each recording and its note here as plain files. Any app that reads Markdown can open them, including an Obsidian vault."
      actions={
        <Button
          variant="primary"
          disabled={busy}
          onClick={() =>
            status.notes_folder ? onNext() : void run(chooseDefaultNotesFolder, true)
          }
        >
          Continue
        </Button>
      }
    >
      <Box>
        <Row
          icon={<FolderIcon />}
          title={folder.display}
          body={folderNote(folder)}
          aside={
            <Button size="sm" disabled={busy} onClick={() => void run(chooseNotesFolder, false)}>
              Choose Folder…
            </Button>
          }
        />
      </Box>
      <p className="mt-3 text-[12px] text-muted">
        To use an existing Obsidian vault, choose the vault folder. You can change this later in
        Settings.
      </p>
      <ErrorLine error={error} />
    </Step>
  );
}

function AudioStep({
  status,
  onStatus,
  onBack,
  onNext,
}: {
  status: SetupStatus;
  onStatus: (next: SetupStatus) => void;
  onBack: () => void;
  onNext: () => void;
}) {
  const [asking, setAsking] = useState<"microphone" | "computer_audio" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(status);
  useEffect(() => {
    latest.current = status;
  });

  async function ask(which: "microphone" | "computer_audio", request: () => Promise<Access>) {
    setAsking(which);
    setError(null);
    try {
      const access = await request();
      onStatus({ ...latest.current, [which]: access });
    } catch (err) {
      setError(message(err));
    } finally {
      setAsking(null);
    }
  }

  // The computer audio prompt may still be open when the one-second check
  // ends. Check again when the user comes back to the window.
  useOnWindowFocus(() => {
    if (latest.current.computer_audio === "denied" && !asking) {
      void ask("computer_audio", () => checkComputerAudio(false));
    }
  });

  const microphone = status.microphone;
  const computerAudio = status.computer_audio;
  const microphoneAnswered = microphone === "allowed" || microphone === "denied";

  return (
    <Step
      step={2}
      title="Allow audio"
      body="Anchovy records only when you press Record or accept a meeting prompt. Audio stays on this Mac."
      actions={
        <>
          <Button variant="ghost" onClick={onBack}>
            Back
          </Button>
          <Button variant="primary" disabled={!microphoneAnswered} onClick={onNext}>
            Continue
          </Button>
        </>
      }
    >
      <Box>
        <Row
          icon={<MicIcon />}
          title="Microphone"
          body="Records your voice. Needed to record."
          aside={
            asking === "microphone" ? (
              <Checking />
            ) : microphone === "allowed" ? (
              <Allowed />
            ) : microphone === "denied" ? (
              <NotAllowed />
            ) : (
              <Button size="sm" onClick={() => void ask("microphone", requestMicrophone)}>
                Allow
              </Button>
            )
          }
        >
          {microphone === "denied" && (
            <DeniedNote pane="microphone">
              Anchovy can't record without the microphone. Allow Anchovy under Microphone in System
              Settings.
            </DeniedNote>
          )}
        </Row>
        <Row
          icon={<SpeakerIcon />}
          title="Computer audio"
          body="Records the other side of calls."
          aside={
            asking === "computer_audio" ? (
              <Checking />
            ) : computerAudio === "allowed" ? (
              <Allowed />
            ) : computerAudio === "denied" ? (
              <NotAllowed />
            ) : (
              <Button
                size="sm"
                disabled={!microphoneAnswered}
                onClick={() => void ask("computer_audio", () => checkComputerAudio(true))}
              >
                Allow
              </Button>
            )
          }
        >
          {computerAudio === "denied" && asking !== "computer_audio" && (
            <DeniedNote pane="computer_audio">
              You can continue. Anchovy will record only your microphone, so the other side of
              online meetings will not be recorded.
            </DeniedNote>
          )}
        </Row>
      </Box>
      <ErrorLine error={error} />
    </Step>
  );
}

// Starts listening for the download of `id` to stop (finished, failed, or
// cancelled). Wrapped in an object: an async function returning a bare
// promise would wait for it.
async function whenStopped(id: string): Promise<{ stopped: Promise<void> }> {
  let resolve!: () => void;
  const stopped = new Promise<void>((done) => (resolve = done));
  const unlisten = await listen<string>("models-changed", (event) => {
    if (event.payload === id) {
      unlisten();
      resolve();
    }
  });
  return { stopped };
}

// Not tied to the screen: after Continue the downloads carry on, one model at
// a time, and the Models screen shows them.
function downloadDefaults(ids: string[], onStart: (id: string) => void) {
  const stopped: Record<string, Promise<void>> = {};
  return downloadInOrder(ids, {
    start: async (id) => {
      stopped[id] = (await whenStopped(id)).stopped;
      onStart(id);
      await downloadModel(id);
    },
    finished: (id) => stopped[id],
  });
}

function ModelsStep({ onDone }: { onDone: () => void }) {
  const [models, setModels] = useState<ModelView[] | null>(null);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [queued, setQueued] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rate, setRate] = useState(0);
  const started = useRef<{ at: number; bytes: number } | null>(null);

  useEffect(() => {
    let live = true;
    const refresh = () =>
      listModels().then(
        (view) => live && setModels(defaultModels(view)),
        (err) => live && setError(message(err)),
      );
    void refresh();
    const unlisten = [
      onModelProgress(({ id, downloaded }) =>
        setProgress((current) => ({ ...current, [id]: downloaded })),
      ),
      onModelsChanged(() => void refresh()),
    ];
    return () => {
      live = false;
      for (const stop of unlisten) void stop.then((fn) => fn());
    };
  }, []);

  const list = models ?? [];
  const { downloaded, total } = totalProgress(list, progress);
  const left = list.filter((model) => model.state.kind !== "ready");
  const bytesLeft = total - downloaded;
  const downloading = queued.length > 0 || list.some((model) => model.state.kind === "downloading");

  // Download speed since Download was pressed, for the time estimate.
  useEffect(() => {
    if (!downloading) return;
    const now = Date.now();
    if (!started.current) {
      started.current = { at: now, bytes: downloaded };
      return;
    }
    const seconds = (now - started.current.at) / 1000;
    if (seconds > 0) setRate((downloaded - started.current.bytes) / seconds);
  }, [downloaded, downloading]);

  async function finish() {
    try {
      await finishSetup();
      onDone();
    } catch (err) {
      setError(message(err));
    }
  }

  function download() {
    const ids = left.map((model) => model.id);
    setQueued(ids);
    void downloadDefaults(ids, setActive).then(() => {
      setQueued([]);
      setActive(null);
    });
  }

  function rowState(model: ModelView) {
    if (model.state.kind === "ready") return "Downloaded";
    if (model.state.kind === "failed") return "Download failed";
    if (model.state.kind === "downloading" || model.id === active) {
      const bytes = downloadedBytes(model, progress);
      return `${(bytes / 1e9).toFixed(1)} of ${formatGb(model.size_bytes)}`;
    }
    if (queued.includes(model.id)) return "Waiting";
    return formatGb(model.size_bytes);
  }

  const estimate = timeLeft(bytesLeft, rate);

  return (
    <Step
      step={3}
      title="Download the models"
      body="Anchovy writes notes with two models that run on this Mac. You can record now and download later in Models."
      actions={
        downloading || (models && left.length === 0) ? (
          <Button variant="primary" onClick={() => void finish()}>
            Continue
          </Button>
        ) : (
          <>
            <Button variant="ghost" onClick={() => void finish()}>
              Later
            </Button>
            <Button variant="primary" disabled={!models} onClick={download}>
              Download {formatGb(bytesLeft)}
            </Button>
          </>
        )
      }
    >
      <Box>
        {list.map((model) => (
          <Row
            key={model.id}
            icon={<ModelsIcon />}
            title={model.display_name}
            body={`${model.role === "transcribe" ? "Transcription" : "Summary"} · ${model.license}`}
            aside={
              <span className="flex h-5 items-center text-[12px] text-muted tabular-nums">
                {rowState(model)}
              </span>
            }
          />
        ))}
      </Box>
      {downloading ? (
        <div className="mt-4">
          <div className="flex justify-between text-[12px] tabular-nums">
            <span>
              Downloading {(downloaded / 1e9).toFixed(1)} of {formatGb(total)}
            </span>
            {estimate && <span className="text-muted">{estimate}</span>}
          </div>
          <Progress value={total ? (downloaded / total) * 100 : 0} className="mt-2" />
          <p className="mt-3 text-[12px] text-muted">
            The download continues in the background. Recordings you make now get their notes when
            it finishes.
          </p>
        </div>
      ) : (
        <p className="mt-3 text-[12px] text-muted tabular-nums">
          Total {formatGb(total)}. Anchovy checks each file after it downloads.
        </p>
      )}
      <ErrorLine error={error} />
    </Step>
  );
}

export function Onboarding({
  initial,
  onDone,
}: {
  initial: SetupStatus;
  onDone: (status: SetupStatus) => void;
}) {
  const [status, setStatus] = useState(initial);
  const [step, setStep] = useState(() => firstStep(initial));

  if (step === 1) {
    return <FolderStep status={status} onChosen={setStatus} onNext={() => setStep(2)} />;
  }
  if (step === 2) {
    return (
      <AudioStep
        status={status}
        onStatus={setStatus}
        onBack={() => setStep(1)}
        onNext={() => setStep(3)}
      />
    );
  }
  return <ModelsStep onDone={() => onDone({ ...status, finished: true })} />;
}
