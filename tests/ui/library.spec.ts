import { expect, test, type Page } from "@playwright/test";
import {
  byFolder,
  defaultCommands,
  emitFakeEvent,
  installFakeIpc,
  type FakeCommands,
} from "./fake-ipc";

// The same recordings and note as the accepted mock in design/mock/src/data.ts.
const recordings = [
  {
    folder: "2026-09-26-1410",
    start: "2026-09-26T14:10",
    duration_seconds: 2520,
    status: "working",
  },
  {
    folder: "2026-09-26-1130",
    start: "2026-09-26T11:30",
    duration_seconds: 1083,
    status: "needs_models",
  },
  {
    folder: "2026-09-26-0905",
    start: "2026-09-26T09:05",
    duration_seconds: 3981,
    status: "failed",
    // As the pipeline writes it; the list shows the first sentence.
    reason:
      "Not enough memory to write the summary. The summary model needs about 3 GB of free memory. Close other apps, then choose Retry.",
  },
  { folder: "2026-09-25-1645", start: "2026-09-25T16:45", duration_seconds: 1634, status: "ready" },
  { folder: "2026-09-25-1000", start: "2026-09-25T10:00", duration_seconds: 552, status: "saved" },
  { folder: "2026-09-23-1520", start: "2026-09-23T15:20", duration_seconds: 3300, status: "ready" },
  { folder: "2026-09-23-0930", start: "2026-09-23T09:30", duration_seconds: 1860, status: "ready" },
];

const note = {
  source: "manual",
  inputs: ["microphone", "computer audio"],
  sections: [
    {
      heading: "Summary",
      body: "The team reviewed the October release. The export fix is done and in testing. The onboarding copy still needs a review, and the release moves from Monday to Wednesday so QA has two more days.",
    },
    {
      heading: "Decisions",
      body: "- Ship the October release on Wednesday, October 7.\n- Keep the old export format for one more release.",
    },
    {
      heading: "Action items",
      body: "- Send the updated release checklist before Friday.\n- Review the onboarding copy with support.\n- Book the QA device lab for Monday and Tuesday.",
    },
    {
      heading: "Transcript",
      body: [
        "00:00:04 Okay, let's start with the release. Where are we on the export fix?",
        "00:00:11 It's merged. QA started this morning and hasn't found anything new so far, but they only covered the PDF path.",
        "00:00:26 Then I'd rather move the date. If we ship Monday, QA gets one day for everything else.",
        "00:00:38 Wednesday works for me. That gives us Monday and Tuesday in the device lab.",
        "00:00:47 Agreed, Wednesday the seventh. And we keep the old export format around for one more release.",
      ].join("\n"),
    },
    { heading: "Audio", body: "[audio.wav](audio.wav)" },
  ],
};

// The shipped defaults, both on this Mac unless a test says otherwise.
const model = (id: string, role: string, name: string, size: number, ready: boolean) => ({
  id,
  role,
  display_name: name,
  languages: [],
  size_bytes: size,
  min_ram_gb: 8,
  license: "Apache-2.0",
  fit: "recommended",
  selected: true,
  state: ready ? { kind: "ready" } : { kind: "not_downloaded" },
});
const models = (summaryReady: boolean) => ({
  memory_bytes: 24 * 1024 ** 3,
  models: [
    model("qwen3-asr-1.7b", "transcribe", "Qwen3-ASR 1.7B", 2_520_744_288, true),
    model(
      "qwen3-4b-instruct-2507-q4",
      "summarize",
      "Qwen3-4B-Instruct-2507",
      2_300_000_000,
      summaryReady,
    ),
  ],
});

// Every recording has its audio, as in the mock: the Saved one was recorded
// Small.
const audio = byFolder(
  Object.fromEntries(
    recordings.map(({ folder }) => {
      const name = folder === "2026-09-25-1000" ? "audio.m4a" : "audio.wav";
      return [folder, { path: `/Users/someone/Documents/Anchovy/${folder}/${name}`, name }];
    }),
  ),
);

// The Working recording is 27 of 42 minutes into its transcript, as in the mock.
const transcribing = {
  folder: "2026-09-26-1410",
  stage: "transcribing",
  done_seconds: 1620,
  total_seconds: 2520,
};

async function open(
  page: Page,
  colorScheme: "light" | "dark",
  list: unknown[],
  commands: FakeCommands = {},
) {
  await page.emulateMedia({ colorScheme });
  // Day headings are relative to today: the mock's "now" is Sep 26, 15:00.
  await page.clock.setFixedTime(new Date(2026, 8, 26, 15, 0));
  await installFakeIpc(page, {
    ...defaultCommands,
    list_recordings: list,
    read_note: note,
    list_models: models(true),
    // The mock's Saved recording is waiting for Generate note.
    get_settings: {
      input_device: null,
      recording_quality: "high",
      generate_notes_automatically: false,
    },
    note_progress: { [transcribing.folder]: transcribing },
    recording_audio: audio,
    ...commands,
  });
  await page.goto("/");
  if (list.length > 0) {
    await expect(page.getByRole("button", { name: /^14:10/ })).toBeVisible();
    await emitFakeEvent(page, "note-progress", transcribing);
  }
}

async function snap(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect(page).toHaveScreenshot(name);
}

// Row for each status, as the mock's note-<status> screens select them.
const selectedFor = {
  working: "14:10",
  "needs-models": "11:30",
  failed: "09:05",
  ready: "16:45",
  saved: "10:00",
} as const;

for (const colorScheme of ["light", "dark"] as const) {
  test(`empty library, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, []);
    await expect(page.getByText("No recordings yet")).toBeVisible();
    // The sources have loaded, as in the mock.
    await expect(page.getByText("Will be recorded")).toBeVisible();
    await expect(page.getByRole("button", { name: "Record" }).first()).toBeEnabled();
    await snap(page, `library-empty-${colorScheme}.png`);
  });

  test(`library with every status, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings);
    await expect(page.getByText("Needs models")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Ready to record" })).toBeVisible();
    await snap(page, `library-${colorScheme}.png`);
  });

  // What each note state must show, as in the mock.
  const shows = {
    working: "27 of 42 min",
    "needs-models": "2.3 GB to download",
    failed: "Retry",
    ready: "Action items",
    saved: "Generate note",
  } as const;

  for (const [status, time] of Object.entries(selectedFor)) {
    test(`${status} recording selected, ${colorScheme}`, async ({ page }) => {
      await open(page, colorScheme, recordings, {
        list_models: models(status !== "needs-models"),
      });
      await page.getByRole("button", { name: new RegExp(`^${time}`) }).click();
      await expect(page.getByRole("heading", { level: 1 })).toContainText(time);
      await expect(page.getByText(shows[status as keyof typeof shows]).first()).toBeVisible();
      // Every one of these recordings has audio, so the player is at the top.
      await expect(page.getByRole("group", { name: "Audio" })).toBeVisible();
      await snap(page, `note-${status}-${colorScheme}.png`);
    });
  }

  test(`regenerate note asks first, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings);
    await page.getByRole("button", { name: /^16:45/ }).click();
    await expect(page.getByText("Action items")).toBeVisible();
    await expect(page.getByRole("group", { name: "Audio" })).toBeVisible();
    await page.getByRole("button", { name: "More actions" }).click();
    await page.getByRole("menuitem", { name: "Regenerate note…" }).click();
    await expect(page.getByRole("alertdialog", { name: "Replace note.md?" })).toBeVisible();
    // The mock shows the dialog without keyboard focus on a button.
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
    await snap(page, `note-regenerate-${colorScheme}.png`);
  });

  test(`more actions menu, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings);
    await page.getByRole("button", { name: /^16:45/ }).click();
    await expect(page.getByRole("group", { name: "Audio" })).toBeVisible();
    await page.getByRole("button", { name: "More actions" }).click();
    await expect(page.getByRole("menuitem", { name: "Move to Trash" })).toBeVisible();
    await snap(page, `note-menu-${colorScheme}.png`);
  });

  // The mock's Settings screen: the defaults, on a MacBook Pro.
  test(`settings, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings, {
      list_input_devices: [
        { uid: "BuiltInMicrophoneDevice", name: "MacBook Pro Microphone", is_default: true },
      ],
      get_settings: {
        input_device: null,
        recording_quality: "high",
        generate_notes_automatically: true,
      },
    });
    await page.getByRole("button", { name: "Settings" }).click();
    await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
    await expect(page.getByRole("combobox", { name: "Input device" })).toHaveText(
      "MacBook Pro Microphone",
    );
    await expect(page.getByRole("switch", { name: "Generate notes automatically" })).toBeEnabled();
    await snap(page, `settings-${colorScheme}.png`);
  });

  // The mock's meeting-banner screen: Zoom has started while the Ready note
  // is open.
  test(`meeting prompt, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings, {
      meeting_prompt: {
        id: 1,
        app: "zoom",
        headline: "Zoom meeting started.",
        body: "Record it? Anchovy records only if you choose Record.",
      },
    });
    await page.getByRole("button", { name: /^16:45/ }).click();
    await expect(page.getByText("Action items")).toBeVisible();
    await expect(page.getByRole("group", { name: "Audio" })).toBeVisible();
    await expect(page.getByRole("status", { name: "Meeting prompt" })).toBeVisible();
    await snap(page, `meeting-banner-${colorScheme}.png`);
  });
}
