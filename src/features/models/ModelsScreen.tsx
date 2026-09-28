import { useCallback, useEffect, useId, useState } from "react";
import {
  cancelModelDownload,
  deleteModel,
  downloadModel,
  listModels,
  onModelProgress,
  onModelsChanged,
  selectModel,
  type Fit,
  type ModelsView,
  type ModelView,
  type Role,
} from "../../ipc/models";

const GIB = 1024 ** 3;

const roles: { role: Role; title: string }[] = [
  { role: "transcribe", title: "Transcription" },
  { role: "summarize", title: "Summary" },
];

const fitLabels: Record<Fit, string> = {
  recommended: "Recommended",
  fits: "Fits",
  too_large: "Too large",
};

function formatSize(bytes: number) {
  return `${(bytes / 1e9).toFixed(1)} GB`;
}

function memoryGb(bytes: number) {
  return Math.round(bytes / GIB);
}

// Rust rejects plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function ModelsScreen() {
  const [data, setData] = useState<ModelsView | null>(null);
  const [progress, setProgress] = useState<Record<string, number>>({});
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    listModels().then(setData, (err) => setError(message(err)));
  }, []);

  useEffect(() => {
    const unlisten = [
      onModelProgress(({ id, downloaded }) =>
        setProgress((current) => ({ ...current, [id]: downloaded })),
      ),
      onModelsChanged(refresh),
    ];
    refresh();
    return () => {
      for (const promise of unlisten) promise.then((stop) => stop());
    };
  }, [refresh]);

  const run = useCallback(
    (action: () => Promise<void>) => {
      setError(null);
      action().then(refresh, (err) => setError(message(err)));
    },
    [refresh],
  );

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-2xl px-8 py-8">
        <header className="mb-6">
          <h1 className="text-lg font-semibold text-text">Models</h1>
          <p className="mt-1 text-sm text-muted">
            Pick one model for each job. Anchovy downloads it and checks every file before using it.
          </p>
          {data && (
            <p className="mt-1 text-sm text-muted">
              This Mac has {memoryGb(data.memory_bytes)} GB of memory.
            </p>
          )}
        </header>
        {error && (
          <p role="alert" className="mb-4 text-sm text-failed">
            {error}
          </p>
        )}
        {data &&
          roles.map(({ role, title }) => (
            <RoleSection
              key={role}
              title={title}
              memoryBytes={data.memory_bytes}
              models={data.models.filter((model) => model.role === role)}
              progress={progress}
              run={run}
            />
          ))}
      </div>
    </div>
  );
}

type RowActions = { run: (action: () => Promise<void>) => void };

function RoleSection({
  title,
  models,
  memoryBytes,
  progress,
  run,
}: {
  title: string;
  models: ModelView[];
  memoryBytes: number;
  progress: Record<string, number>;
} & RowActions) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="mb-8">
      <h2 id={headingId} className="mb-2 text-xs font-medium tracking-wide text-muted uppercase">
        {title}
      </h2>
      <div className="divide-y divide-line rounded-lg border border-line">
        {models.map((model) => (
          <ModelRow
            key={model.id}
            model={model}
            memoryBytes={memoryBytes}
            liveDownloaded={progress[model.id]}
            run={run}
          />
        ))}
      </div>
    </section>
  );
}

type Confirm = "select" | "download" | "delete" | null;

function ModelRow({
  model,
  memoryBytes,
  liveDownloaded,
  run,
}: {
  model: ModelView;
  memoryBytes: number;
  liveDownloaded: number | undefined;
} & RowActions) {
  const nameId = useId();
  const [confirm, setConfirm] = useState<Confirm>(null);
  const tooLarge = model.fit === "too_large";
  const name = model.display_name;

  const select = () => {
    setConfirm(null);
    run(() => selectModel(model.id));
  };
  const download = () => {
    setConfirm(null);
    run(() => downloadModel(model.id));
  };
  const remove = () => {
    setConfirm(null);
    run(() => deleteModel(model.id));
  };

  return (
    <div role="group" aria-labelledby={nameId} className="flex gap-3 px-4 py-3">
      <input
        type="radio"
        name={model.role}
        aria-label={`Use ${name}`}
        checked={model.selected}
        onChange={() => (tooLarge ? setConfirm("select") : select())}
        className="mt-1 self-start accent-selected"
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span id={nameId} className="text-sm font-medium text-text">
            {name}
          </span>
          <span
            className={`rounded border px-1.5 text-[11px] leading-4 ${
              tooLarge ? "border-failed/40 text-failed" : "border-line text-muted"
            }`}
          >
            {fitLabels[model.fit]}
          </span>
          <span className="ml-auto text-xs text-muted tabular-nums">
            {formatSize(model.size_bytes)}
          </span>
        </div>
        <p className="mt-0.5 text-xs text-muted">
          {model.languages.join(", ")} · {model.license}
        </p>
        {confirm ? (
          <ConfirmLine
            text={
              confirm === "delete"
                ? `Delete ${name} from this Mac? You can download it again later.`
                : `${name} needs ${model.min_ram_gb} GB of memory. This Mac has ${memoryGb(
                    memoryBytes,
                  )} GB, so notes may fail or take much longer.`
            }
            action={
              confirm === "delete"
                ? "Delete"
                : confirm === "select"
                  ? "Use anyway"
                  : "Download anyway"
            }
            onConfirm={confirm === "delete" ? remove : confirm === "select" ? select : download}
            onCancel={() => setConfirm(null)}
          />
        ) : (
          <StatusLine
            model={model}
            liveDownloaded={liveDownloaded}
            onDownload={() => (tooLarge ? setConfirm("download") : download())}
            onCancel={() => run(() => cancelModelDownload(model.id))}
            onDelete={() => setConfirm("delete")}
          />
        )}
      </div>
    </div>
  );
}

function StatusLine({
  model,
  liveDownloaded,
  onDownload,
  onCancel,
  onDelete,
}: {
  model: ModelView;
  liveDownloaded: number | undefined;
  onDownload: () => void;
  onCancel: () => void;
  onDelete: () => void;
}) {
  const { state, size_bytes: size } = model;
  switch (state.kind) {
    case "not_downloaded":
      return (
        <Line>
          <span className="text-muted">Not downloaded</span>
          <Button onClick={onDownload}>Download</Button>
        </Line>
      );
    case "downloading": {
      const downloaded = Math.max(state.downloaded, liveDownloaded ?? 0);
      const percent = Math.round((downloaded * 100) / size);
      return (
        <Line>
          <div className="flex min-w-0 flex-1 items-center gap-3">
            <div
              role="progressbar"
              aria-label={`Downloading ${model.display_name}`}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent}
              className="h-1 flex-1 overflow-hidden rounded-full bg-line"
            >
              <div className="h-full bg-selected" style={{ width: `${percent}%` }} />
            </div>
            <span className="text-muted tabular-nums">
              {formatSize(downloaded)} of {formatSize(size)}
            </span>
          </div>
          <Button onClick={onCancel}>Cancel</Button>
        </Line>
      );
    }
    case "paused":
      return (
        <Line>
          <span className="text-muted tabular-nums">
            {formatSize(state.downloaded)} of {formatSize(size)} downloaded
          </span>
          <Button onClick={onDownload}>Resume</Button>
        </Line>
      );
    case "failed":
      return (
        <Line>
          <span className="text-failed">{state.error}</span>
          <Button onClick={onDownload}>Retry</Button>
        </Line>
      );
    case "ready":
      return (
        <Line>
          <span className="text-ready">Downloaded</span>
          <Button onClick={onDelete}>Delete</Button>
        </Line>
      );
  }
}

function ConfirmLine({
  text,
  action,
  onConfirm,
  onCancel,
}: {
  text: string;
  action: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="mt-2 rounded-md bg-surface-raised px-3 py-2 text-xs">
      <p className="text-text">{text}</p>
      <div className="mt-2 flex justify-end gap-2">
        <Button onClick={onCancel}>Cancel</Button>
        <Button primary onClick={onConfirm}>
          {action}
        </Button>
      </div>
    </div>
  );
}

function Line({ children }: { children: React.ReactNode }) {
  return (
    <div className="mt-2 flex min-h-7 items-center justify-between gap-3 text-xs">{children}</div>
  );
}

function Button({
  primary = false,
  onClick,
  children,
}: {
  primary?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`shrink-0 rounded-md px-2.5 py-1 text-xs font-medium ${
        primary
          ? "bg-text text-surface hover:opacity-90"
          : "border border-line text-text hover:bg-surface-raised"
      }`}
    >
      {children}
    </button>
  );
}
