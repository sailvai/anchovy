import path from "node:path";
import { expect, test } from "@playwright/test";
import { screens, themes, windowSize } from "./src/catalog";

// Renders every mock screen in light and dark, checks it, and writes the
// screenshot the app is compared against from step 2b on.
for (const screen of screens) {
  for (const theme of themes) {
    test(`${screen.id}, ${theme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: theme });
      await page.goto(`/?screen=${screen.id}&theme=${theme}`);
      await page.evaluate(() => document.fonts.ready);

      await expect(page.getByText(screen.text, { exact: false }).first()).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);

      // Nothing spills outside the window.
      const overflow = await page.evaluate(() => {
        const root = document.scrollingElement!;
        return { width: root.scrollWidth, height: root.scrollHeight };
      });
      expect(overflow.width).toBeLessThanOrEqual(windowSize.width);
      expect(overflow.height).toBeLessThanOrEqual(windowSize.height);

      // Fonts come from the app bundle, never from the network.
      const fonts = await page.evaluate(() =>
        [...document.fonts].filter((font) => font.status === "loaded").map((font) => font.family),
      );
      expect(fonts).toContain("Geist Variable");

      await page.screenshot({
        path: path.join(import.meta.dirname, "screenshots", `${screen.id}-${theme}.png`),
        animations: "disabled",
      });
    });
  }
}

test("gallery links every screen in both themes", async ({ page }) => {
  await page.goto("/");
  for (const screen of screens) {
    for (const theme of themes) {
      await expect(page.locator(`a[href="?screen=${screen.id}&theme=${theme}"]`)).toHaveCount(1);
    }
  }
});

test("mock makes no network requests outside the dev server", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (request) => {
    if (!request.url().startsWith("http://localhost:1440/")) external.push(request.url());
  });
  for (const screen of screens) await page.goto(`/?screen=${screen.id}&theme=light`);
  expect(external).toEqual([]);
});

test("meeting prompt does not look like a recording is running", async ({ page }) => {
  await page.goto("/?screen=meeting-banner&theme=light");
  const banner = page.getByRole("status");
  const red = banner.locator(".bg-recording");
  await expect(red).toHaveCount(1);
  await expect(banner.getByRole("button", { name: "Record" }).locator(".bg-recording")).toHaveCount(
    1,
  );
  await expect(page.getByText("Recording", { exact: true })).toHaveCount(0);
});
