import type { ReactNode, SVGProps } from "react";

// Line icons from the design mock, so the app needs no icon package. 16 px,
// 1.5 stroke.
function Svg({
  children,
  className = "",
  ...props
}: SVGProps<SVGSVGElement> & { children: ReactNode }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={`shrink-0 ${className}`}
      {...props}
    >
      {children}
    </svg>
  );
}

type IconProps = SVGProps<SVGSVGElement>;

export function MicIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="5.5" y="1.75" width="5" height="8" rx="2.5" />
      <path d="M3.25 7.5a4.75 4.75 0 0 0 9.5 0M8 12.25v2" />
    </Svg>
  );
}

export function SpeakerIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.25 6h2.5l3.5-3v10l-3.5-3h-2.5z" />
      <path d="M10.75 5.5a3.5 3.5 0 0 1 0 5M12.5 3.75a6 6 0 0 1 0 8.5" />
    </Svg>
  );
}

export function RecordDot({ className = "" }: { className?: string }) {
  return <span className={`inline-block size-2 rounded-full bg-recording ${className}`} />;
}

export function MoreIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="3.5" cy="8" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.4" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r="1.4" fill="currentColor" stroke="none" />
    </Svg>
  );
}

export function ModelsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 1.75 13.5 4.75v6.5L8 14.25 2.5 11.25v-6.5z" />
      <path d="M2.5 4.75 8 7.75l5.5-3M8 7.75v6.5" />
    </Svg>
  );
}

export function SettingsIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.5 4.5h6M11.5 4.5h2M2.5 11.5h2M7.5 11.5h6" />
      <circle cx="10" cy="4.5" r="1.5" />
      <circle cx="6" cy="11.5" r="1.5" />
    </Svg>
  );
}

export function AlertIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="8" r="6.25" />
      <path d="M8 4.75v3.75" />
      <circle cx="8" cy="11" r=".5" fill="currentColor" />
    </Svg>
  );
}

export function DownloadIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 2.25v8M4.75 7.25 8 10.5l3.25-3.25M2.75 13.25h10.5" />
    </Svg>
  );
}

export function FinderIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M8 2.5v11M5 6v1M11 6v1M5.5 10a3.5 3.5 0 0 0 5 0" />
    </Svg>
  );
}

export function TrashIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.75 4.25h10.5M6.25 4.25V2.75h3.5v1.5M4 4.25l.6 8.6c.04.52.47.9.99.9h4.82c.52 0 .95-.38.99-.9l.6-8.6" />
    </Svg>
  );
}

export function Spinner({ className = "" }: { className?: string }) {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 16 16"
      fill="none"
      className={`animate-spin ${className}`}
      aria-hidden="true"
    >
      <circle cx="8" cy="8" r="6" stroke="currentColor" strokeOpacity=".25" strokeWidth="2" />
      <path d="M14 8a6 6 0 0 0-6-6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
    </svg>
  );
}
