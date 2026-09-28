import type { ReactNode } from "react";
import { type Recording, recordings as allRecordings } from "../data";
import { MeetingIcon, ModelsIcon, RecordDot, SettingsIcon } from "../icons";
import { Button, StatusLabel } from "../ui";

export type Place = "models" | "settings" | undefined;

// One window: recordings on the left, the current item on the right.
export function Window({
  recordings = allRecordings,
  selected,
  place,
  recording = false,
  banner,
  children,
}: {
  recordings?: Recording[];
  selected?: string;
  place?: Place;
  recording?: boolean;
  banner?: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="relative flex h-full flex-col">
      {banner}
      <div className="flex min-h-0 flex-1">
        <Sidebar recordings={recordings} selected={selected} place={place} recording={recording} />
        <main className="min-w-0 flex-1 bg-surface">{children}</main>
      </div>
    </div>
  );
}

function Sidebar({
  recordings,
  selected,
  place,
  recording,
}: {
  recordings: Recording[];
  selected?: string;
  place?: Place;
  recording: boolean;
}) {
  const days = [...new Set(recordings.map((item) => item.day))];
  return (
    <aside className="flex w-[240px] shrink-0 flex-col border-r border-line bg-surface-raised">
      <div className="p-3">
        <Button className="w-full" disabled={recording}>
          <RecordDot />
          Record
        </Button>
      </div>
      <nav className="min-h-0 flex-1 overflow-hidden px-2" aria-label="Recordings">
        {recordings.length === 0 ? (
          <p className="px-2 pt-4 text-center text-[12px] text-muted">No recordings yet</p>
        ) : (
          days.map((day) => (
            <section key={day} className="pb-2">
              <h2 className="px-2 pt-2 pb-1 text-[11px] font-medium text-muted">{day}</h2>
              {recordings
                .filter((item) => item.day === day)
                .map((item) => (
                  <Row key={item.folder} item={item} selected={item.folder === selected} />
                ))}
            </section>
          ))
        )}
      </nav>
      <div className="border-t border-line p-2">
        <NavItem icon={<ModelsIcon />} label="Models" active={place === "models"} />
        <NavItem icon={<SettingsIcon />} label="Settings" active={place === "settings"} />
      </div>
    </aside>
  );
}

function Row({ item, selected }: { item: Recording; selected: boolean }) {
  return (
    <div
      aria-current={selected || undefined}
      className={`rounded-md px-2 py-1.5 ${selected ? "bg-selected" : "hover:bg-hover"}`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <span className="font-medium tabular-nums">{item.time}</span>
        <span className="text-[12px] text-muted tabular-nums">{item.duration}</span>
      </div>
      <StatusLabel status={item.status} detail={item.detail} />
    </div>
  );
}

function NavItem({ icon, label, active }: { icon: ReactNode; label: string; active: boolean }) {
  return (
    <div
      aria-current={active || undefined}
      className={`flex h-8 items-center gap-2 rounded-md px-2 ${active ? "bg-selected text-text" : "text-muted hover:bg-hover hover:text-text"}`}
    >
      {icon}
      <span className="font-medium">{label}</span>
    </div>
  );
}

// Meeting prompt. It sits above the window content and never blocks it.
export function MeetingBanner({ app }: { app: string }) {
  return (
    <div
      role="status"
      className="flex h-12 shrink-0 items-center gap-3 border-b border-line bg-surface-raised px-4"
    >
      <MeetingIcon className="shrink-0 text-muted" />
      <p className="min-w-0 flex-1 truncate">
        <span className="font-medium">{app} meeting started.</span>{" "}
        <span className="text-muted">Record it? Anchovy records only if you choose Record.</span>
      </p>
      <Button size="sm" variant="ghost">
        Not now
      </Button>
      <Button size="sm" variant="primary">
        <RecordDot />
        Record
      </Button>
    </div>
  );
}

export function PaneHeader({
  title,
  subtitle,
  actions,
}: {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="flex min-h-16 items-center gap-3 border-b border-line px-6 py-3">
      <div className="min-w-0 flex-1">
        <h1 className="truncate text-[15px] font-semibold tabular-nums">{title}</h1>
        {subtitle && <p className="truncate text-[12px] text-muted">{subtitle}</p>}
      </div>
      {actions}
    </header>
  );
}
