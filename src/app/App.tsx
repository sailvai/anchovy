import { useEffect, useState } from "react";
import { ModelsScreen } from "../features/models/ModelsScreen";
import { appInfo, type AppInfo } from "../ipc/commands";

type Screen = "home" | "models";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [screen, setScreen] = useState<Screen>("home");

  useEffect(() => {
    appInfo().then(setInfo, () => setInfo(null));
  }, []);

  const showingModels = screen === "models";

  return (
    <div className="flex h-screen flex-col bg-surface text-text">
      <main className="min-h-0 flex-1">{showingModels && <ModelsScreen />}</main>
      <footer className="flex items-center justify-between border-t border-line px-4 py-2 text-muted">
        <button
          type="button"
          aria-pressed={showingModels}
          onClick={() => setScreen(showingModels ? "home" : "models")}
          className={`rounded-md px-2 py-1 text-xs font-medium hover:bg-surface-raised ${
            showingModels ? "bg-surface-raised text-text" : ""
          }`}
        >
          Models
        </button>
        {info && (
          <p className="text-xs tabular-nums">
            {info.name} {info.version}
          </p>
        )}
      </footer>
    </div>
  );
}
