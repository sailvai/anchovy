// Fake data for the mock. Nothing here comes from a real recording, device,
// or model file. Sizes are placeholders until the shipped list in step 5.

export type Status = "recording" | "saved" | "needs-models" | "working" | "ready" | "failed";

export const statusLabel: Record<Status, string> = {
  recording: "Recording",
  saved: "Saved",
  "needs-models": "Needs models",
  working: "Working",
  ready: "Ready",
  failed: "Failed",
};

export type Recording = {
  folder: string;
  day: string;
  time: string;
  duration: string;
  status: Status;
  detail?: string;
};

// Newest first, grouped by day in the list.
export const recordings: Recording[] = [
  {
    folder: "2026-09-26-1410",
    day: "Today",
    time: "14:10",
    duration: "42 min",
    status: "working",
    detail: "Transcribing",
  },
  {
    folder: "2026-09-26-1130",
    day: "Today",
    time: "11:30",
    duration: "18 min",
    status: "needs-models",
  },
  {
    folder: "2026-09-26-0905",
    day: "Today",
    time: "09:05",
    duration: "1 h 06 min",
    status: "failed",
    detail: "Not enough memory",
  },
  {
    folder: "2026-09-25-1645",
    day: "Yesterday",
    time: "16:45",
    duration: "27 min",
    status: "ready",
  },
  {
    folder: "2026-09-25-1000",
    day: "Yesterday",
    time: "10:00",
    duration: "9 min",
    status: "saved",
  },
  {
    folder: "2026-09-23-1520",
    day: "Wed, Sep 23",
    time: "15:20",
    duration: "55 min",
    status: "ready",
  },
  {
    folder: "2026-09-23-0930",
    day: "Wed, Sep 23",
    time: "09:30",
    duration: "31 min",
    status: "ready",
  },
];

export const liveRecording: Recording = {
  folder: "2026-09-26-1502",
  day: "Today",
  time: "15:02",
  duration: "12 min",
  status: "recording",
};

export const microphoneName = "MacBook Pro Microphone";
export const inputDevices = [microphoneName, "Studio Display Microphone", "AirPods Pro"];
export const notesFolder = "~/Documents/Anchovy";

export type Model = {
  name: string;
  detail: string;
  size: string;
  license: string;
  isDefault?: boolean;
};

export const transcriptionModels: Model[] = [
  {
    name: "Qwen3-ASR 1.7B",
    detail: "Best accuracy. Many languages.",
    size: "3.4 GB",
    license: "Apache 2.0",
    isDefault: true,
  },
  {
    name: "Qwen3-ASR 0.6B",
    detail: "Faster, for Macs with 8 GB of memory.",
    size: "1.3 GB",
    license: "Apache 2.0",
  },
];

export const summaryModels: Model[] = [
  {
    name: "Qwen3-4B-Instruct-2507",
    detail: "4-bit. Clear summaries and action items.",
    size: "2.3 GB",
    license: "Apache 2.0",
    isDefault: true,
  },
  {
    name: "Qwen3-1.7B",
    detail: "4-bit. Smaller and faster, shorter summaries.",
    size: "1.1 GB",
    license: "Apache 2.0",
  },
];

// The body of 2026-09-25-1645/note.md. The screen shows the same text.
export const note = {
  heading: "2026-09-25 16:45",
  duration: "27 min",
  source: "Manual",
  inputs: "Microphone, computer audio",
  audio: "audio.wav",
  asrModel: "Qwen3-ASR 1.7B",
  summaryModel: "Qwen3-4B-Instruct-2507",
  summary:
    "The team reviewed the October release. The export fix is done and in testing. The onboarding copy still needs a review, and the release moves from Monday to Wednesday so QA has two more days.",
  decisions: [
    "Ship the October release on Wednesday, October 7.",
    "Keep the old export format for one more release.",
  ],
  actionItems: [
    "Send the updated release checklist before Friday.",
    "Review the onboarding copy with support.",
    "Book the QA device lab for Monday and Tuesday.",
  ],
  transcript: [
    ["00:00:04", "Okay, let's start with the release. Where are we on the export fix?"],
    [
      "00:00:11",
      "It's merged. QA started this morning and hasn't found anything new so far, but they only covered the PDF path.",
    ],
    [
      "00:00:26",
      "Then I'd rather move the date. If we ship Monday, QA gets one day for everything else.",
    ],
    ["00:00:38", "Wednesday works for me. That gives us Monday and Tuesday in the device lab."],
    [
      "00:00:47",
      "Agreed, Wednesday the seventh. And we keep the old export format around for one more release.",
    ],
  ],
};
