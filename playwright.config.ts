import { defineConfig, devices } from "@playwright/test";

// Screenshot tests render the interface in a browser with fake Rust commands.
// Baselines live next to the tests and change only with a reviewed pull request.
export default defineConfig({
  testDir: "tests/ui",
  snapshotPathTemplate: "{testDir}/__screenshots__/{testFilePath}/{arg}{ext}",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  expect: {
    toHaveScreenshot: { maxDiffPixelRatio: 0.01, animations: "disabled" },
  },
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1000, height: 680 },
    baseURL: "http://localhost:1430",
  },
  webServer: {
    command: "npx vite preview --port 1430 --strictPort",
    url: "http://localhost:1430",
    reuseExistingServer: !process.env.CI,
  },
});
