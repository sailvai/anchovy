import { expect, test } from "@playwright/test";
import { installFakeIpc } from "./fake-ipc";

for (const colorScheme of ["light", "dark"] as const) {
  test(`empty window, ${colorScheme}`, async ({ page }) => {
    await page.emulateMedia({ colorScheme });
    await installFakeIpc(page);
    await page.goto("/");
    await expect(page.getByText("Anchovy 0.1.0")).toBeVisible();
    await page.evaluate(() => document.fonts.ready);
    await expect(page).toHaveScreenshot(`empty-window-${colorScheme}.png`);
  });
}
