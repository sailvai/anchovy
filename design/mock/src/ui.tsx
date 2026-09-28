import type { ButtonHTMLAttributes } from "react";
import { type Status, statusLabel } from "./data";
import { AlertIcon, ChevronDownIcon, RecordDot, Spinner } from "./icons";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost";
  size?: "sm" | "md" | "icon";
};

// One primary button per flow. Everything else is secondary or ghost.
export function Button({
  variant = "secondary",
  size = "md",
  className = "",
  ...props
}: ButtonProps) {
  const sizes = {
    sm: "h-7 px-2.5 text-[12px]",
    md: "h-8 px-3 text-[13px]",
    icon: "size-8",
  };
  const variants = {
    primary: "bg-primary text-primary-fg hover:bg-primary/90",
    secondary: "border border-line-strong bg-surface text-text hover:bg-hover",
    ghost: "text-muted hover:bg-hover hover:text-text",
  };
  return (
    <button
      type="button"
      className={`inline-flex shrink-0 items-center justify-center gap-1.5 rounded-md font-medium whitespace-nowrap disabled:opacity-40 ${sizes[size]} ${variants[variant]} ${className}`}
      {...props}
    />
  );
}

const statusColor: Record<Status, string> = {
  recording: "text-recording",
  saved: "text-muted",
  "needs-models": "text-attention",
  working: "text-accent",
  ready: "text-ready",
  failed: "text-failed",
};

// Settled states keep a colored dot but gray text, so the list stays quiet and
// only states that need attention stand out.
const quiet: Status[] = ["ready", "saved"];

function StatusMark({ status }: { status: Status }) {
  if (status === "recording") return <RecordDot className="size-[7px]" />;
  if (status === "working") return <Spinner className="size-[10px]" />;
  if (status === "failed") return <AlertIcon className="size-[11px]" />;
  const fill = status === "saved" ? "bg-faint" : "bg-current";
  return <span className={`inline-block size-[7px] rounded-full ${fill}`} />;
}

export function StatusLabel({ status, detail }: { status: Status; detail?: string }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-[12px]">
      <span className={`flex w-3 shrink-0 justify-center ${statusColor[status]}`}>
        <StatusMark status={status} />
      </span>
      <span className={`shrink-0 ${quiet.includes(status) ? "text-muted" : statusColor[status]}`}>
        {statusLabel[status]}
      </span>
      {detail && <span className="truncate text-muted">· {detail}</span>}
    </span>
  );
}

export function Toggle({ on }: { on: boolean }) {
  return (
    <span
      role="switch"
      aria-checked={on}
      className={`relative inline-flex h-[18px] w-8 shrink-0 rounded-full transition-colors ${on ? "bg-accent" : "bg-line-strong"}`}
    >
      <span
        className={`absolute top-[2px] size-[14px] rounded-full bg-white shadow-sm transition-all ${on ? "left-[16px]" : "left-[2px]"}`}
      />
    </span>
  );
}

export function Radio({ checked }: { checked: boolean }) {
  return (
    <span
      role="radio"
      aria-checked={checked}
      className={`flex size-4 shrink-0 items-center justify-center rounded-full border ${checked ? "border-accent" : "border-line-strong"}`}
    >
      {checked && <span className="size-2 rounded-full bg-accent" />}
    </span>
  );
}

export function Select({ value, className = "" }: { value: string; className?: string }) {
  return (
    <span
      className={`inline-flex h-8 items-center justify-between gap-2 rounded-md border border-line-strong bg-surface pr-2 pl-3 text-[13px] ${className}`}
    >
      <span className="truncate">{value}</span>
      <ChevronDownIcon className="size-3.5 shrink-0 text-muted" />
    </span>
  );
}

export function Progress({ value, className = "" }: { value: number; className?: string }) {
  return (
    <span className={`block h-1 overflow-hidden rounded-full bg-line ${className}`}>
      <span className="block h-full rounded-full bg-accent" style={{ width: `${value}%` }} />
    </span>
  );
}

// Input level while recording. Fixed bar heights, since the mock is static.
export function LevelMeter({ levels, muted = false }: { levels: number[]; muted?: boolean }) {
  return (
    <span className="flex h-4 items-center gap-[2px]" aria-label="Input level">
      {levels.map((level, index) => (
        <span
          key={index}
          className={`w-[3px] rounded-full ${muted ? "bg-line-strong" : "bg-muted"}`}
          style={{ height: `${Math.max(2, level * 16)}px` }}
        />
      ))}
    </span>
  );
}
