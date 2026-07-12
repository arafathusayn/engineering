// Test fixtures: a shared page server, and a `page` that records V8
// coverage of the inline app.js. Coverage is persisted to disk per page so
// it survives Playwright worker restarts; coverage.spec.mjs merges and
// asserts 100% at the end of the run.
import { test as base } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { startServer, APP_JS } from "./server.mjs";

export const COVERAGE_DIR = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  ".coverage"
);
export const ERRORS_FILE = path.join(COVERAGE_DIR, "console-errors.log");

let seq = 0;

async function collect(page, testInfo) {
  const entries = await page.coverage.stopJSCoverage();
  for (const entry of entries) {
    // The inline script whose source IS templates/app.js.
    if (entry.source !== APP_JS) continue;
    fs.mkdirSync(COVERAGE_DIR, { recursive: true });
    const name = `${testInfo.workerIndex}-${process.pid}-${seq++}.json`;
    fs.writeFileSync(
      path.join(COVERAGE_DIR, name),
      JSON.stringify({ functions: entry.functions })
    );
  }
}

export const test = base.extend({
  server: [
    // eslint-disable-next-line no-empty-pattern
    async ({}, use) => {
      const { server, origin } = await startServer();
      await use(origin);
      await new Promise((r) => server.close(r));
    },
    { scope: "worker" },
  ],

  page: async ({ page }, use, testInfo) => {
    const logError = (text) => {
      fs.mkdirSync(COVERAGE_DIR, { recursive: true });
      fs.appendFileSync(ERRORS_FILE, `${testInfo.title}: ${text}\n`);
    };
    page.on("pageerror", (e) => logError(String(e)));
    page.on("console", (m) => {
      if (m.type() === "error") logError(m.text());
    });
    await page.coverage.startJSCoverage({ resetOnNavigation: false });
    await use(page);
    await collect(page, testInfo);
  },
});

export { expect } from "@playwright/test";
