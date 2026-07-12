import { defineConfig } from "@playwright/test";

// CHROMIUM_PATH: use a pre-installed Chromium instead of the revision this
// @playwright/test version would download (e.g. sandboxed environments with
// PLAYWRIGHT_SKIP_BROWSER_DOWNLOAD=1). CI installs the matching browser and
// leaves it unset.
const executablePath = process.env.CHROMIUM_PATH;

export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/global-setup.ts",
  // One worker: every test merges V8 coverage of templates/app.js into one
  // on-disk collection, and the final test asserts 100%.
  workers: 1,
  fullyParallel: false,
  timeout: 30_000,
  reporter: [["list"]],
  use: {
    browserName: "chromium",
    launchOptions: executablePath ? { executablePath } : {},
  },
});
