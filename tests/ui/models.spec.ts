import { expect, test } from "@playwright/test";
import { defaultCommands, installFakeIpc } from "./fake-ipc";

const GB = 1e9;

// An 8 GB Mac with one model in each state.
const models = {
  memory_bytes: 8 * 1024 ** 3,
  models: [
    {
      id: "qwen3-asr-1.7b",
      role: "transcribe",
      display_name: "Qwen3-ASR 1.7B",
      languages: ["Chinese", "English", "Cantonese", "Japanese", "Korean"],
      size_bytes: 2.52 * GB,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "fits",
      selected: true,
      state: { kind: "ready" },
    },
    {
      id: "qwen3-asr-0.6b",
      role: "transcribe",
      display_name: "Qwen3-ASR 0.6B",
      languages: ["Chinese", "English", "Cantonese", "Japanese", "Korean"],
      size_bytes: 1.02 * GB,
      min_ram_gb: 4,
      license: "Apache-2.0",
      fit: "recommended",
      selected: false,
      state: { kind: "downloading", downloaded: 0.41 * GB },
    },
    {
      id: "qwen3-4b-instruct-2507-q4",
      role: "summarize",
      display_name: "Qwen3-4B Instruct 2507",
      languages: ["Chinese", "English", "Japanese", "Korean", "French"],
      size_bytes: 2.5 * GB,
      min_ram_gb: 8,
      license: "Apache-2.0",
      fit: "fits",
      selected: true,
      state: {
        kind: "failed",
        downloaded: 1.1 * GB,
        error: "The download was interrupted. Check the connection and resume.",
      },
    },
    {
      id: "qwen3-4b-instruct-2507-q8",
      role: "summarize",
      display_name: "Qwen3-4B Instruct 2507 (8-bit)",
      languages: ["Chinese", "English", "Japanese", "Korean", "French"],
      size_bytes: 4.28 * GB,
      min_ram_gb: 16,
      license: "Apache-2.0",
      fit: "too_large",
      selected: false,
      state: { kind: "not_downloaded" },
    },
  ],
};

for (const colorScheme of ["light", "dark"] as const) {
  test(`models screen, ${colorScheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await installFakeIpc(page, { ...defaultCommands, list_models: models });
    await page.goto("/");
    await page.getByRole("button", { name: "Models" }).click();
    await expect(page.getByText("This Mac has 8 GB of memory.")).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await expect(page).toHaveScreenshot(`models-${colorScheme}.png`);
  });

  test(`models screen asking before a too large model, ${colorScheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await installFakeIpc(page, { ...defaultCommands, list_models: models });
    await page.goto("/");
    await page.getByRole("button", { name: "Models" }).click();
    await page.getByRole("radio", { name: "Use Qwen3-4B Instruct 2507 (8-bit)" }).click();
    await expect(page.getByRole("button", { name: "Use anyway" })).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await expect(page).toHaveScreenshot(`models-too-large-${colorScheme}.png`);
  });
}
