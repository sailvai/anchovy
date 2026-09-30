import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { listInputDevices, type InputDevice } from "../../ipc/recording";
import { getSettings, updateSettings, type Quality, type Settings } from "../../ipc/settings";
import { chooseNotesFolder, type NotesFolder } from "../../ipc/setup";
import { ChevronDownIcon, FolderIcon } from "../library/icons";
import { Button } from "../library/ui";

// Rust rejects with plain strings; show whatever it sent.
function message(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

const qualities: { value: Quality; label: string; detail: string }[] = [
  { value: "high", label: "High", detail: "WAV, 48 kHz, 16-bit, mono" },
  { value: "small", label: "Small", detail: "M4A" },
];

// The menu's first entry stands for the system default: no device is saved.
const SYSTEM_DEFAULT = "";

// Settings replace the right side, like Models. Exactly four: the notes
// folder, the input device, the recording quality, and automatic notes.
// Device and quality apply from the next recording; Rust reads them when a
// recording starts.
export function SettingsScreen({
  notesFolder,
  recording,
  onNotesFolderChanged,
  onSaved,
}: {
  notesFolder: NotesFolder | null;
  // Change… is off while recording: the recording is written in that folder.
  recording: boolean;
  onNotesFolderChanged: (folder: NotesFolder) => void;
  onSaved?: (settings: Settings) => void;
}) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [choosing, setChoosing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The settings with every change so far, saved or on its way, so a second
  // change made before the first is saved keeps the first.
  const latest = useRef<Settings | null>(null);

  useEffect(() => {
    let current = true;
    getSettings().then(
      (saved) => {
        if (!current) return;
        latest.current = saved;
        setSettings(saved);
      },
      (err) => current && setError(message(err)),
    );
    listInputDevices().then(
      (list) => current && setDevices(list),
      () => {},
    );
    return () => {
      current = false;
    };
  }, []);

  function save(change: Partial<Settings>) {
    const before = latest.current;
    if (!before) return;
    const next = { ...before, ...change };
    latest.current = next;
    setSettings(next);
    setError(null);
    updateSettings(next).then(
      (saved) => {
        if (latest.current === next) setSettings(saved);
        onSaved?.(saved);
      },
      (err) => {
        // Not saved: back to what was there before this change.
        if (latest.current === next) {
          latest.current = before;
          setSettings(before);
        }
        setError(message(err));
      },
    );
  }

  async function changeFolder() {
    setChoosing(true);
    setError(null);
    try {
      const chosen = await chooseNotesFolder();
      if (chosen) onNotesFolderChanged(chosen);
    } catch (err) {
      setError(message(err));
    } finally {
      setChoosing(false);
    }
  }

  // The system default first, named as macOS names it, then the others in
  // system order. A saved device that is not connected shows the default,
  // which is what Record uses, and stays saved until it is back.
  const systemDefault = devices.find((device) => device.is_default);
  const others = devices.filter((device) => !device.is_default);
  const selectedDevice = others.some((device) => device.uid === settings?.input_device)
    ? (settings?.input_device ?? SYSTEM_DEFAULT)
    : SYSTEM_DEFAULT;

  return (
    <div className="flex h-full flex-col">
      <header className="flex min-h-16 items-center gap-3 border-b border-line px-6 py-3">
        <h1 className="truncate text-[15px] font-semibold">Settings</h1>
      </header>
      {error && (
        <p role="alert" className="border-b border-line px-6 py-2 text-[12px] text-failed">
          {error}
        </p>
      )}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5">
        <div className="max-w-[620px] divide-y divide-line rounded-lg border border-line">
          <Row label="Notes folder" help="Each recording gets its own folder here.">
            <div className="flex min-w-0 items-center gap-2">
              <span className="flex min-w-0 items-center gap-1.5 text-muted">
                <FolderIcon className="shrink-0" />
                <span className="truncate" title={notesFolder?.path}>
                  {notesFolder?.display}
                </span>
              </span>
              <Button size="sm" disabled={recording || choosing} onClick={changeFolder}>
                Change…
              </Button>
            </div>
          </Row>
          <Row label="Input device" help="Mixed with computer audio when allowed.">
            {(labelId) => (
              <span className="relative inline-flex w-[220px]">
                <select
                  aria-labelledby={labelId}
                  value={selectedDevice}
                  disabled={!settings}
                  onChange={(event) => save({ input_device: event.target.value || null })}
                  className="h-8 w-full appearance-none truncate rounded-md border border-line-strong bg-surface pr-7 pl-3 text-[13px] text-text disabled:opacity-40"
                >
                  <option value={SYSTEM_DEFAULT}>{systemDefault?.name ?? "System default"}</option>
                  {others.map((device) => (
                    <option key={device.uid} value={device.uid}>
                      {device.name}
                    </option>
                  ))}
                </select>
                <ChevronDownIcon className="pointer-events-none absolute top-1/2 right-2 size-3.5 -translate-y-1/2 text-muted" />
              </span>
            )}
          </Row>
          <Row label="Recording quality" help="Applies to the next recording.">
            {(labelId) => (
              <div role="radiogroup" aria-labelledby={labelId} className="w-[220px] space-y-2">
                {qualities.map(({ value, label, detail }) => (
                  <QualityOption
                    key={value}
                    label={label}
                    detail={detail}
                    checked={settings?.recording_quality === value}
                    disabled={!settings}
                    onChange={() => save({ recording_quality: value })}
                  />
                ))}
              </div>
            )}
          </Row>
          <Row label="Generate notes automatically" help="When off, use Generate note.">
            {(labelId) => (
              <Toggle
                labelledBy={labelId}
                on={settings?.generate_notes_automatically ?? true}
                disabled={!settings}
                onChange={(on) => save({ generate_notes_automatically: on })}
              />
            )}
          </Row>
        </div>
      </div>
    </div>
  );
}

function Row({
  label,
  help,
  children,
}: {
  label: string;
  help: string;
  children: ReactNode | ((labelId: string) => ReactNode);
}) {
  const labelId = useId();
  return (
    <div role="group" aria-label={label} className="flex items-start gap-6 px-4 py-3.5">
      <div className="w-[250px] shrink-0">
        <p id={labelId} className="font-medium">
          {label}
        </p>
        <p className="text-[12px] text-muted">{help}</p>
      </div>
      <div className="flex min-w-0 flex-1 justify-end">
        {typeof children === "function" ? children(labelId) : children}
      </div>
    </div>
  );
}

function QualityOption({
  label,
  detail,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  detail: string;
  checked: boolean;
  disabled: boolean;
  onChange: () => void;
}) {
  return (
    <label className="flex items-center gap-2.5">
      <input
        type="radio"
        name="recording-quality"
        className="peer sr-only"
        checked={checked}
        disabled={disabled}
        onChange={onChange}
      />
      <span
        aria-hidden="true"
        className={`flex size-4 shrink-0 items-center justify-center rounded-full border peer-focus-visible:ring-2 peer-focus-visible:ring-accent/40 ${checked ? "border-accent" : "border-line-strong"}`}
      >
        {checked && <span className="size-2 rounded-full bg-accent" />}
      </span>
      <span>
        <span className="font-medium">{label}</span>{" "}
        <span className="text-[12px] text-muted">{detail}</span>
      </span>
    </label>
  );
}

function Toggle({
  labelledBy,
  on,
  disabled,
  onChange,
}: {
  labelledBy: string;
  on: boolean;
  disabled: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={on}
      aria-labelledby={labelledBy}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative inline-flex h-[18px] w-8 shrink-0 rounded-full transition-colors disabled:opacity-40 ${on ? "bg-accent" : "bg-line-strong"}`}
    >
      <span
        className={`absolute top-[2px] size-[14px] rounded-full bg-white shadow-sm transition-all ${on ? "left-[16px]" : "left-[2px]"}`}
      />
    </button>
  );
}
