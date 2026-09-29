import type { ReactNode } from "react";
import { ModelsIcon, RecordDot, SettingsIcon } from "./icons";
import type { Stage } from "../../ipc/notes";
import { formatDuration, groupByDay, timeOfDay, type Recording } from "./library";
import { headline, stageLabel } from "./notes";
import { Button, StatusLabel } from "./ui";

export type Place = "models" | "settings";

// Left side of the window: Record, the recordings newest first, then Models
// and Settings.
export function Sidebar({
  recordings,
  selected,
  stages = {},
  place,
  now,
  canRecord,
  onRecord,
  onSelect,
  onPlace,
}: {
  recordings: Recording[] | null;
  selected: string | null;
  // Where each Working note is, by folder.
  stages?: Record<string, Stage>;
  place: Place | null;
  now: Date;
  canRecord: boolean;
  onRecord: () => void;
  onSelect: (folder: string) => void;
  onPlace: (place: Place) => void;
}) {
  return (
    <aside className="flex w-[240px] shrink-0 flex-col border-r border-line bg-surface-raised">
      <div className="p-3">
        <Button className="w-full" disabled={!canRecord} onClick={onRecord}>
          <RecordDot />
          Record
        </Button>
      </div>
      <nav className="min-h-0 flex-1 overflow-y-auto px-2" aria-label="Recordings">
        {recordings?.length === 0 && (
          <p className="px-2 pt-4 text-center text-[12px] text-muted">No recordings yet</p>
        )}
        {recordings &&
          groupByDay(recordings, now).map(({ day, items }) => (
            <section key={day} className="pb-2">
              <h2 className="px-2 pt-2 pb-1 text-[11px] font-medium text-muted">{day}</h2>
              {items.map((item) => (
                <Row
                  key={item.folder}
                  item={item}
                  stage={stages[item.folder]}
                  selected={item.folder === selected}
                  onSelect={() => onSelect(item.folder)}
                />
              ))}
            </section>
          ))}
      </nav>
      <div className="border-t border-line p-2">
        <NavItem
          icon={<ModelsIcon />}
          label="Models"
          active={place === "models"}
          onClick={() => onPlace("models")}
        />
        {/* Settings arrive in plan step 8. */}
        <NavItem icon={<SettingsIcon />} label="Settings" active={place === "settings"} />
      </div>
    </aside>
  );
}

function Row({
  item,
  stage,
  selected,
  onSelect,
}: {
  item: Recording;
  stage: Stage | undefined;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-current={selected || undefined}
      onClick={onSelect}
      className={`block w-full rounded-md px-2 py-1.5 text-left ${selected ? "bg-highlight" : "hover:bg-hover"}`}
    >
      <span className="flex items-baseline justify-between gap-2">
        <span className="font-medium tabular-nums">{timeOfDay(item.start)}</span>
        <span className="text-[12px] text-muted tabular-nums">
          {formatDuration(item.duration_seconds)}
        </span>
      </span>
      <StatusLabel
        status={item.status}
        detail={
          item.status === "failed"
            ? headline(item.reason)
            : item.status === "working" && stage
              ? stageLabel(stage)
              : undefined
        }
      />
    </button>
  );
}

function NavItem({
  icon,
  label,
  active,
  onClick,
}: {
  icon: ReactNode;
  label: string;
  active: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={!onClick}
      onClick={onClick}
      className={`flex h-8 w-full items-center gap-2 rounded-md px-2 disabled:opacity-40 ${active ? "bg-highlight text-text" : "text-muted enabled:hover:bg-hover enabled:hover:text-text"}`}
    >
      {icon}
      <span className="font-medium">{label}</span>
    </button>
  );
}
