import { expect, test, type Page } from "@playwright/test";
import { defaultCommands, emitFakeEvent, installFakeIpc } from "./fake-ipc";

// Record, the recording pane, and computer audio that is not allowed, as in
// the accepted mock's recording and library-mic-only screens.
const live = {
  folder: "2026-09-26-1502",
  start: "2026-09-26T15:02",
  duration_seconds: null,
  status: "recording",
};

const earlier = [
  {
    folder: "2026-09-26-1410",
    start: "2026-09-26T14:10",
    duration_seconds: 2520,
    status: "working",
  },
  { folder: "2026-09-25-1645", start: "2026-09-25T16:45", duration_seconds: 1634, status: "ready" },
];

async function open(page: Page, colorScheme: "light" | "dark", commands: Record<string, unknown>) {
  await page.emulateMedia({ colorScheme });
  await page.clock.setFixedTime(new Date(2026, 8, 26, 15, 15));
  await installFakeIpc(page, { ...defaultCommands, ...commands });
  await page.goto("/");
}

async function snap(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect(page).toHaveScreenshot(name);
}

for (const colorScheme of ["light", "dark"] as const) {
  test(`recording, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, {
      list_recordings: [live, ...earlier],
      start_recording: {
        folder: `/Users/someone/Documents/Anchovy/${live.folder}`,
        microphone: "MacBook Air Microphone",
        computer_audio: "recording",
        quality: "high",
      },
    });
    await page.getByRole("button", { name: "Record" }).first().click();
    await expect(page.getByRole("button", { name: "Stop" })).toBeVisible();
    await emitFakeEvent(page, "recording-progress", { seconds: 768, bytes: 73_728_044 });
    await expect(page.getByLabel("Elapsed time")).toHaveText("00:12:48");
    await expect(page.getByText("73.7 MB · WAV, 48 kHz, 16-bit, mono")).toBeVisible();
    await snap(page, `recording-${colorScheme}.png`);
  });

  test(`computer audio not allowed, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, {
      list_recordings: earlier,
      check_computer_audio: "denied",
      setup_status: { ...(defaultCommands.setup_status as object), computer_audio: "denied" },
    });
    await expect(page.getByText("Not allowed")).toBeVisible();
    await snap(page, `library-mic-only-${colorScheme}.png`);
  });
}
