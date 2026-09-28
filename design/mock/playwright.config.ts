import { defineConfig, devices } from "@playwright/test";
import { windowSize } from "./src/catalog";

// Screenshots of the design mock. Run from the repository root:
//   npx playwright test -c design/mock
export default defineConfig({
  testDir: ".",
  testMatch: "mock.spec.ts",
  fullyParallel: true,
  retries: 0,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: windowSize,
    deviceScaleFactor: 2,
    baseURL: "http://localhost:1440",
  },
  webServer: {
    command: "npx vite -c design/mock/vite.config.ts",
    cwd: "../..",
    url: "http://localhost:1440",
    reuseExistingServer: true,
  },
});
