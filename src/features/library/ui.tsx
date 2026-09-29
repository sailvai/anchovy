import type { ComponentProps } from "react";
import { AlertIcon, RecordDot, Spinner } from "./icons";
import { type Status, statusLabel } from "./library";

type ButtonProps = ComponentProps<"button"> & {
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
    primary: "bg-primary text-primary-fg enabled:hover:bg-primary/90",
    secondary: "border border-line-strong bg-surface text-text enabled:hover:bg-hover",
    ghost: "text-muted enabled:hover:bg-hover enabled:hover:text-text",
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
  needs_models: "text-attention",
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
    <span className="inline-flex max-w-full min-w-0 items-center gap-1.5 text-[12px]">
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

export function Progress({ value, className = "" }: { value: number; className?: string }) {
  return (
    <span
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(value)}
      className={`block h-1 overflow-hidden rounded-full bg-line ${className}`}
    >
      <span className="block h-full rounded-full bg-accent" style={{ width: `${value}%` }} />
    </span>
  );
}
