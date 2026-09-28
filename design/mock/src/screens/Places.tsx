import type { ReactNode } from "react";
import {
  type Model,
  microphoneName,
  notesFolder,
  summaryModels,
  transcriptionModels,
} from "../data";
import { AlertIcon, CheckIcon, FolderIcon } from "../icons";
import { Button, Progress, Radio, Select, Toggle } from "../ui";
import { PaneHeader } from "./Window";

// Models and Settings replace the right side; they never open a window.

type ModelState =
  | { kind: "ready" }
  | { kind: "available" }
  | { kind: "downloading"; done: string; percent: number }
  | { kind: "failed"; reason: string };

function ModelRow({
  model,
  selected,
  state,
}: {
  model: Model;
  selected: boolean;
  state: ModelState;
}) {
  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <Radio checked={selected} />
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-2">
          <span className="font-medium">{model.name}</span>
          {model.isDefault && (
            <span className="rounded border border-line px-1.5 text-[11px] text-muted">
              Default
            </span>
          )}
        </p>
        <p className="text-[12px] text-muted">
          {model.detail} {model.size} · {model.license}
        </p>
        {state.kind === "downloading" && (
          <div className="mt-2 flex items-center gap-3">
            <Progress value={state.percent} className="flex-1" />
            <span className="text-[12px] text-muted tabular-nums">{state.done}</span>
          </div>
        )}
        {state.kind === "failed" && (
          <p className="mt-1 flex items-center gap-1.5 text-[12px] text-failed">
            <AlertIcon className="size-3" />
            {state.reason}
          </p>
        )}
      </div>
      <div className="flex w-28 justify-end">
        {state.kind === "ready" && (
          <span className="inline-flex items-center gap-1 text-[12px] text-ready">
            <CheckIcon className="size-3.5" />
            On this Mac
          </span>
        )}
        {state.kind === "available" && <Button size="sm">Download</Button>}
        {state.kind === "downloading" && (
          <Button size="sm" variant="ghost">
            Cancel
          </Button>
        )}
        {state.kind === "failed" && <Button size="sm">Retry</Button>}
      </div>
    </div>
  );
}

function Group({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="mt-6 first:mt-0">
      <h2 className="text-[12px] font-medium text-muted">{title}</h2>
      <div className="mt-2 divide-y divide-line rounded-lg border border-line">{children}</div>
      {note && <p className="mt-2 text-[12px] text-muted">{note}</p>}
    </section>
  );
}

export function ModelsPane() {
  return (
    <div className="flex h-full flex-col">
      <PaneHeader
        title="Models"
        subtitle="Recommended models that run on this Mac. Anchovy downloads, checks, and loads them."
      />
      <div className="min-h-0 flex-1 overflow-hidden px-6 py-5">
        <div className="max-w-[620px]">
          <Group title="Transcription">
            <ModelRow model={transcriptionModels[0]} selected state={{ kind: "ready" }} />
            <ModelRow
              model={transcriptionModels[1]}
              selected={false}
              state={{ kind: "available" }}
            />
          </Group>
          <Group title="Summary">
            <ModelRow
              model={summaryModels[0]}
              selected
              state={{ kind: "downloading", done: "1.4 of 2.3 GB", percent: 61 }}
            />
            <ModelRow
              model={summaryModels[1]}
              selected={false}
              state={{ kind: "failed", reason: "Download stopped. The connection was lost." }}
            />
          </Group>
          <p className="mt-6 text-[12px] text-muted">
            Anchovy loads one model at a time: it transcribes, unloads that model, then writes the
            summary. Changing a model does not rewrite existing notes. To use the new model on a
            recording, choose Regenerate note.
          </p>
        </div>
      </div>
    </div>
  );
}

function Setting({ label, help, children }: { label: string; help: string; children: ReactNode }) {
  return (
    <div className="flex items-start gap-6 px-4 py-3.5">
      <div className="w-[250px] shrink-0">
        <p className="font-medium">{label}</p>
        <p className="text-[12px] text-muted">{help}</p>
      </div>
      <div className="flex min-w-0 flex-1 justify-end">{children}</div>
    </div>
  );
}

function QualityOption({
  label,
  detail,
  checked,
}: {
  label: string;
  detail: string;
  checked: boolean;
}) {
  return (
    <div className="flex items-center gap-2.5">
      <Radio checked={checked} />
      <span>
        <span className="font-medium">{label}</span>{" "}
        <span className="text-[12px] text-muted">{detail}</span>
      </span>
    </div>
  );
}

export function SettingsPane() {
  return (
    <div className="flex h-full flex-col">
      <PaneHeader title="Settings" />
      <div className="min-h-0 flex-1 overflow-hidden px-6 py-5">
        <div className="max-w-[620px] divide-y divide-line rounded-lg border border-line">
          <Setting label="Notes folder" help="Each recording gets its own folder here.">
            <div className="flex min-w-0 items-center gap-2">
              <span className="flex min-w-0 items-center gap-1.5 text-muted">
                <FolderIcon className="shrink-0" />
                <span className="truncate">{notesFolder}</span>
              </span>
              <Button size="sm">Change…</Button>
            </div>
          </Setting>
          <Setting label="Input device" help="Mixed with computer audio when allowed.">
            <Select value={microphoneName} className="w-[220px]" />
          </Setting>
          <Setting label="Recording quality" help="Applies to the next recording.">
            <div className="w-[220px] space-y-2">
              <QualityOption label="High" detail="WAV, 48 kHz, 16-bit, mono" checked />
              <QualityOption label="Small" detail="M4A" checked={false} />
            </div>
          </Setting>
          <Setting label="Generate notes automatically" help="When off, use Generate note.">
            <Toggle on />
          </Setting>
        </div>
      </div>
    </div>
  );
}
