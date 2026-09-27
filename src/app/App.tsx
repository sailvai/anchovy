import { useEffect, useState } from "react";
import { appInfo, type AppInfo } from "../ipc/commands";

export function App() {
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    appInfo().then(setInfo, () => setInfo(null));
  }, []);

  return (
    <main className="flex h-screen items-end justify-end bg-surface p-4 text-muted">
      {info && (
        <p className="text-xs tabular-nums">
          {info.name} {info.version}
        </p>
      )}
    </main>
  );
}
