import { useCallback, useEffect, useState } from "react";
import { HomePane } from "../features/library/HomePane";
import {
  folderName,
  listRecordings,
  moveToTrash,
  showInFinder,
  type Recording,
} from "../features/library/library";
import { NotePane } from "../features/library/NotePane";
import { RecordingPane } from "../features/library/RecordingPane";
import { Sidebar, type Place } from "../features/library/Sidebar";
import type { ComputerAudioRow } from "../features/library/Sources";
import { ModelsScreen } from "../features/models/ModelsScreen";
import { Onboarding } from "../features/setup/Onboarding";
import { useOnWindowFocus } from "../features/setup/useOnWindowFocus";
import {
  onRecordingProgress,
  recordingSources,
  startRecording,
  stopRecording,
  type RecordingProgress,
} from "../ipc/recording";
import {
  checkComputerAudio,
  openPrivacySettings,
  setupStatus,
  type SetupStatus,
} from "../ipc/setup";

// What the right side shows: nothing selected, one recording, or a place that
// replaces it.
type View =
  { kind: "home" } | { kind: "recording"; folder: string } | { kind: "place"; place: Place };

// The recording in progress, by folder name, with what it is capturing.
type Live = {
  folder: string;
  microphone: string;
  computerAudio: ComputerAudioRow;
  progress: RecordingProgress;
};

// Rust rejects plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

// Why Record is off, when it is.
function blockedReason(setup: SetupStatus): string | null {
  if (setup.can_record) return null;
  if (!setup.notes_folder) return "Choose a notes folder to record.";
  return "Anchovy needs microphone access to record. Allow it in System Settings.";
}

// One window: recordings on the left, the current item on the right. Until
// first launch is done (or when the notes folder is gone), the first-launch
// screens fill the window.
export function App() {
  const [setup, setSetup] = useState<SetupStatus | null>(null);
  const [recordings, setRecordings] = useState<Recording[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ kind: "home" });
  const [microphone, setMicrophone] = useState<string | null>(null);
  const [computerAudio, setComputerAudio] = useState<ComputerAudioRow>("unchecked");
  const [live, setLive] = useState<Live | null>(null);
  const [busy, setBusy] = useState(false);
  const [recordError, setRecordError] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      listRecordings().then(
        (list) => {
          setRecordings(list);
          setError(null);
        },
        (err) => setError(String(err)),
      ),
    [],
  );

  const loadSetup = useCallback(
    () =>
      setupStatus().then(
        (status) => {
          setSetup(status);
          return status;
        },
        (err) => {
          setError(String(err));
          return null;
        },
      ),
    [],
  );

  // What Record will use: the microphone's name, and whether computer audio
  // is allowed (a one-second check once it has been asked for). A check that
  // fails means computer audio cannot be recorded either.
  const loadSources = useCallback(
    () =>
      Promise.all([
        recordingSources().catch(() => null),
        checkComputerAudio(false).catch((): ComputerAudioRow => "denied"),
      ]).then(([sources, access]) => {
        setMicrophone(sources?.microphone ?? null);
        setComputerAudio(access);
      }),
    [],
  );

  const inWindow = setup !== null && setup.finished && setup.notes_folder !== null;

  useEffect(() => {
    void loadSetup();
  }, [loadSetup]);

  useEffect(() => {
    if (!inWindow) return;
    void refresh();
    void loadSources();
  }, [inWindow, refresh, loadSources]);

  // Coming back from System Settings: the microphone or computer audio may
  // now be allowed.
  useOnWindowFocus(() => {
    if (!inWindow || live) return;
    void loadSetup();
    if (computerAudio === "denied") {
      void checkComputerAudio(false).then(setComputerAudio, () => {});
    }
  });

  // Elapsed time and file size while recording.
  const recording = live !== null;
  useEffect(() => {
    if (!recording) return;
    const unlisten = onRecordingProgress((progress) =>
      setLive((current) => current && { ...current, progress }),
    );
    return () => void unlisten.then((fn) => fn());
  }, [recording]);

  async function record() {
    setBusy(true);
    setRecordError(null);
    try {
      const started = await startRecording();
      const folder = folderName(started.folder);
      setLive({
        folder,
        microphone: started.microphone,
        computerAudio: started.computer_audio === "recording" ? "allowed" : "denied",
        progress: { seconds: 0, bytes: 0 },
      });
      setView({ kind: "recording", folder });
      await refresh();
    } catch (err) {
      setRecordError(`Anchovy couldn't start recording. ${message(err)}`);
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    setRecordError(null);
    try {
      const saved = await stopRecording();
      setView({ kind: "recording", folder: folderName(saved.folder) });
    } catch (err) {
      // The audio up to the error is kept and saved.
      setRecordError(`The recording stopped with an error. ${message(err)}`);
    } finally {
      setLive(null);
      setBusy(false);
      await refresh();
    }
  }

  async function allowComputerAudio() {
    if (computerAudio === "not_asked") {
      setComputerAudio("checking");
      try {
        setComputerAudio(await checkComputerAudio(true));
      } catch {
        setComputerAudio("not_asked");
      }
    } else {
      // macOS shows its prompt only once; after that it is a setting.
      await openPrivacySettings("computer_audio");
    }
  }

  if (!setup) {
    return (
      <div className="h-screen bg-surface text-text">
        {error && (
          <p role="alert" className="px-4 py-2 text-[12px] text-failed">
            {error}
          </p>
        )}
      </div>
    );
  }

  if (!inWindow) {
    return (
      <div className="h-screen bg-surface text-text">
        <Onboarding
          initial={setup}
          onDone={() => {
            void loadSetup();
          }}
        />
      </div>
    );
  }

  const selected =
    view.kind === "recording"
      ? (recordings?.find((item) => item.folder === view.folder) ?? null)
      : null;
  const place = view.kind === "place" ? view.place : null;
  const showLive =
    live && (view.kind === "home" || (view.kind === "recording" && view.folder === live.folder));

  let pane;
  if (place === "models") {
    pane = <ModelsScreen />;
  } else if (showLive) {
    pane = (
      <RecordingPane
        folder={live.folder}
        microphone={live.microphone}
        computerAudio={live.computerAudio}
        seconds={live.progress.seconds}
        bytes={live.progress.bytes}
        stopping={busy}
        error={recordError}
        onStop={() => void stop()}
      />
    );
  } else if (selected) {
    pane = (
      <NotePane
        key={selected.folder}
        recording={selected}
        onShowInFinder={() => showInFinder(selected.folder)}
        onMoveToTrash={async () => {
          await moveToTrash(selected.folder);
          setView({ kind: "home" });
          await refresh();
        }}
        onDownloadModels={() => setView({ kind: "place", place: "models" })}
      />
    );
  } else {
    pane = (
      <HomePane
        microphone={microphone}
        computerAudio={computerAudio}
        canRecord={setup.can_record && !busy && !live}
        blocked={blockedReason(setup)}
        error={recordError}
        onRecord={() => void record()}
        onAllowComputerAudio={() => void allowComputerAudio()}
      />
    );
  }

  return (
    <div className="flex h-screen flex-col bg-surface text-text">
      {error && (
        <p role="alert" className="border-b border-line px-4 py-2 text-[12px] text-failed">
          Anchovy can't read the notes folder. {error}
        </p>
      )}
      <div className="flex min-h-0 flex-1">
        <Sidebar
          recordings={recordings}
          selected={showLive ? live.folder : (selected?.folder ?? null)}
          place={place}
          now={new Date()}
          canRecord={setup.can_record && !busy && !live}
          onRecord={() => void record()}
          onSelect={(folder) => setView({ kind: "recording", folder })}
          onPlace={(next) =>
            setView(next === place ? { kind: "home" } : { kind: "place", place: next })
          }
        />
        <main className="min-w-0 flex-1 bg-surface">{pane}</main>
      </div>
    </div>
  );
}
