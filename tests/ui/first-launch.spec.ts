import { expect, test, type Page } from "@playwright/test";
import { defaultCommands, emitFakeEvent, installFakeIpc, notesFolder } from "./fake-ipc";

// The first-launch screens, named like the accepted mock's screenshots in
// design/mock/screenshots/. Model sizes are the shipped list's, so the totals
// differ from the mock's placeholder numbers.
const GB = 1e9;

const newFolder = { ...notesFolder, exists: false };

const fresh = {
  notes_folder: null,
  default_folder: newFolder,
  microphone: "allowed",
  computer_audio: "not_asked",
  finished: false,
  can_record: false,
};

const models = {
  memory_bytes: 16 * 1024 ** 3,
  models: [
    {
      id: "qwen3-asr-1.7b",
      role: "transcribe",
      display_name: "Qwen3-ASR 1.7B",
      languages: ["Chinese", "English", "Cantonese", "Japanese", "Korean"],
      size_bytes: 2.52 * GB,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "recommended",
      selected: true,
      state: { kind: "not_downloaded" },
    },
    {
      id: "qwen3-asr-0.6b",
      role: "transcribe",
      display_name: "Qwen3-ASR 0.6B",
      languages: ["Chinese", "English", "Cantonese", "Japanese", "Korean"],
      size_bytes: 1.02 * GB,
      min_ram_gb: 4,
      license: "Apache-2.0",
      fit: "fits",
      selected: false,
      state: { kind: "not_downloaded" },
    },
    {
      id: "qwen3-4b-instruct-2507-q4",
      role: "summarize",
      display_name: "Qwen3-4B Instruct 2507",
      languages: ["Chinese", "English", "Japanese", "Korean", "French"],
      size_bytes: 2.5 * GB,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "recommended",
      selected: true,
      state: { kind: "not_downloaded" },
    },
  ],
};

const start = new Date(2026, 8, 26, 15, 0);

async function open(page: Page, colorScheme: "light" | "dark", commands: Record<string, unknown>) {
  await page.emulateMedia({ colorScheme });
  await page.clock.setFixedTime(start);
  await installFakeIpc(page, {
    ...defaultCommands,
    list_models: models,
    use_default_notes_folder: notesFolder,
    download_model: null,
    finish_setup: null,
    open_privacy_settings: null,
    ...commands,
  });
  await page.goto("/");
}

async function snap(page: Page, name: string) {
  await page.evaluate(() => document.fonts.ready);
  await expect(page).toHaveScreenshot(name);
}

for (const colorScheme of ["light", "dark"] as const) {
  test(`notes folder, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, { setup_status: fresh });
    await expect(page.getByRole("heading", { name: "Choose a notes folder" })).toBeVisible();
    await expect(page.getByText("~/Documents/Anchovy")).toBeVisible();
    await snap(page, `onboarding-folder-${colorScheme}.png`);
  });

  test(`audio access, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, { setup_status: fresh });
    await page.getByRole("button", { name: "Continue" }).click();
    await expect(page.getByRole("heading", { name: "Allow audio" })).toBeVisible();
    await expect(page.getByText("Allowed")).toBeVisible();
    await snap(page, `onboarding-audio-${colorScheme}.png`);
  });

  test(`computer audio denied, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, { setup_status: fresh, check_computer_audio: "denied" });
    await page.getByRole("button", { name: "Continue" }).click();
    await page.getByRole("button", { name: "Allow" }).click();
    await expect(page.getByText("will not be recorded")).toBeVisible();
    await snap(page, `onboarding-audio-denied-${colorScheme}.png`);
  });

  test(`models, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, {
      setup_status: { ...fresh, notes_folder: notesFolder, computer_audio: "allowed" },
    });
    await expect(page.getByRole("heading", { name: "Download the models" })).toBeVisible();
    await expect(page.getByRole("button", { name: "Download 5.0 GB" })).toBeVisible();
    await snap(page, `onboarding-models-${colorScheme}.png`);
  });

  test(`models downloading, ${colorScheme}`, async ({ page }) => {
    await open(page, colorScheme, {
      setup_status: { ...fresh, notes_folder: notesFolder, computer_audio: "allowed" },
    });
    await page.getByRole("button", { name: "Download 5.0 GB" }).click();
    await expect(page.getByText("Waiting")).toBeVisible();
    // 1.6 GB in 200 seconds leaves 3.4 GB: 7.1 minutes, rounded up.
    await page.clock.setFixedTime(new Date(start.getTime() + 200_000));
    await emitFakeEvent(page, "model-progress", { id: "qwen3-asr-1.7b", downloaded: 1.6 * GB });
    await expect(page.getByText("Downloading 1.6 of 5.0 GB")).toBeVisible();
    await expect(page.getByText("About 8 minutes left")).toBeVisible();
    await snap(page, `onboarding-models-downloading-${colorScheme}.png`);
  });
}
