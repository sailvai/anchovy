// Every screen in the mock. `text` must be visible when the screen renders;
// the screenshot test checks it in light and dark.
export const screens = [
  { id: "onboarding-folder", title: "First launch 1: notes folder", text: "Choose a notes folder" },
  { id: "onboarding-audio", title: "First launch 2: audio access", text: "Allow audio" },
  {
    id: "onboarding-audio-denied",
    title: "First launch 2: computer audio denied",
    text: "will not be recorded",
  },
  { id: "onboarding-models", title: "First launch 3: models", text: "Download the models" },
  {
    id: "onboarding-models-downloading",
    title: "First launch 3: downloading",
    text: "Downloading 2.1 of 5.7 GB",
  },
  { id: "library-empty", title: "Library, empty", text: "No recordings yet" },
  { id: "library", title: "Library, every status", text: "Needs models" },
  {
    id: "library-mic-only",
    title: "Library, computer audio not allowed",
    text: "Not allowed",
  },
  { id: "recording", title: "Recording in progress", text: "Stop" },
  { id: "note-saved", title: "Recording saved, no note yet", text: "Generate note" },
  { id: "note-needs-models", title: "Needs models", text: "Download models" },
  { id: "note-working", title: "Working", text: "Transcribing" },
  { id: "note-failed", title: "Failed", text: "Retry" },
  { id: "note-ready", title: "Note", text: "Action items" },
  { id: "note-menu", title: "Note, more actions", text: "Move to Trash" },
  { id: "note-regenerate", title: "Regenerate note, confirm", text: "Replace note.md?" },
  { id: "models", title: "Models", text: "Transcription" },
  { id: "settings", title: "Settings", text: "Generate notes automatically" },
  { id: "meeting-banner", title: "Meeting prompt", text: "Zoom meeting started" },
] as const;

export type ScreenId = (typeof screens)[number]["id"];

export const themes = ["light", "dark"] as const;
export type Theme = (typeof themes)[number];

// The app window's default size in src-tauri/tauri.conf.json.
export const windowSize = { width: 1000, height: 680 };
