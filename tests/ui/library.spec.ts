import { expect, test, type Page } from "@playwright/test";
import { defaultCommands, installFakeIpc } from "./fake-ipc";

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
    reason: "Not enough memory",
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

async function open(page: Page, colorScheme: "light" | "dark", list: unknown[]) {
  await page.emulateMedia({ colorScheme });
  // Day headings are relative to today: the mock's "now" is Sep 26, 15:00.
  await page.clock.setFixedTime(new Date(2026, 8, 26, 15, 0));
  await installFakeIpc(page, { ...defaultCommands, list_recordings: list, read_note: note });
  await page.goto("/");
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
    await snap(page, `library-empty-${colorScheme}.png`);
  });

  test(`library with every status, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings);
    await expect(page.getByText("Needs models")).toBeVisible();
    await expect(page.getByRole("heading", { name: "Ready to record" })).toBeVisible();
    await snap(page, `library-${colorScheme}.png`);
  });

  for (const [status, time] of Object.entries(selectedFor)) {
    test(`${status} recording selected, ${colorScheme}`, async ({ page }) => {
      await open(page, colorScheme, recordings);
      await page.getByRole("button", { name: new RegExp(`^${time}`) }).click();
      await expect(page.getByRole("heading", { level: 1 })).toContainText(time);
      if (status === "ready") await expect(page.getByText("Action items")).toBeVisible();
      await snap(page, `note-${status}-${colorScheme}.png`);
    });
  }

  test(`more actions menu, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, recordings);
    await page.getByRole("button", { name: /^16:45/ }).click();
    await page.getByRole("button", { name: "More actions" }).click();
    await expect(page.getByRole("menuitem", { name: "Move to Trash" })).toBeVisible();
    await snap(page, `note-menu-${colorScheme}.png`);
  });
}
