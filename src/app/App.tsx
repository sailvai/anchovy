import { useCallback, useEffect, useState } from "react";
import { HomePane } from "../features/library/HomePane";
import {
  listRecordings,
  moveToTrash,
  showInFinder,
  type Recording,
} from "../features/library/library";
import { NotePane } from "../features/library/NotePane";
import { Sidebar, type Place } from "../features/library/Sidebar";
import { ModelsScreen } from "../features/models/ModelsScreen";

// What the right side shows: nothing selected, one recording, or a place that
// replaces it.
type View =
  { kind: "home" } | { kind: "recording"; folder: string } | { kind: "place"; place: Place };

// One window: recordings on the left, the current item on the right.
export function App() {
  const [recordings, setRecordings] = useState<Recording[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ kind: "home" });

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

  useEffect(() => {
    refresh();
  }, [refresh]);

  const selected =
    view.kind === "recording"
      ? (recordings?.find((item) => item.folder === view.folder) ?? null)
      : null;
  const place = view.kind === "place" ? view.place : null;

  let pane;
  if (place === "models") {
    pane = <ModelsScreen />;
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
    pane = <HomePane />;
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
          selected={selected?.folder ?? null}
          place={place}
          now={new Date()}
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
