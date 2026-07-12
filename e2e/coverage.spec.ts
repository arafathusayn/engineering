// Runs after app.spec.ts (alphabetical order, single worker): merges the
// V8 coverage every test persisted and asserts templates/app.js is 100%
// covered — statements, lines, functions, and branches — and that no test
// produced a page or console error.
import { test, expect } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import v8toIstanbul from "v8-to-istanbul";
import libCoverage from "istanbul-lib-coverage";
import { COVERAGE_DIR, ERRORS_FILE } from "./fixtures";
import { APP_JS } from "./server";

const APP_JS_PATH = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "templates",
  "app.js"
);

test("app.js is 100% covered and no test saw console errors", async () => {
  const errors = fs.existsSync(ERRORS_FILE) ? fs.readFileSync(ERRORS_FILE, "utf8") : "";
  expect(errors, "no page or console errors in any test").toBe("");

  const dumps = fs.readdirSync(COVERAGE_DIR).filter((f) => f.endsWith(".json"));
  expect(dumps.length, "coverage was recorded by the app tests").toBeGreaterThan(0);

  const map = libCoverage.createCoverageMap({});
  for (const dump of dumps) {
    const { functions } = JSON.parse(fs.readFileSync(path.join(COVERAGE_DIR, dump), "utf8"));
    const converter = v8toIstanbul(APP_JS_PATH, 0, { source: APP_JS });
    await converter.load();
    converter.applyCoverage(functions);
    map.merge(converter.toIstanbul());
  }

  const fileCov = map.fileCoverageFor(map.files()[0]);
  const lines = APP_JS.split("\n");

  const uncovered = fileCov.getUncoveredLines();
  expect(
    uncovered.length,
    "uncovered lines:\n" + uncovered.map((n) => `  L${n}: ${lines[n - 1]?.trim()}`).join("\n")
  ).toBe(0);

  const missedBranches: string[] = [];
  for (const [id, counts] of Object.entries(fileCov.b)) {
    counts.forEach((count, i) => {
      if (count === 0) {
        const loc = fileCov.branchMap[id].locations[i]?.start;
        missedBranches.push(`L${loc?.line}: ${lines[(loc?.line ?? 1) - 1]?.trim()} (arm ${i})`);
      }
    });
  }
  expect(missedBranches, "all branches taken both ways").toEqual([]);

  const s = fileCov.toSummary().toJSON();
  expect(s.statements.pct).toBe(100);
  expect(s.functions.pct).toBe(100);
  expect(s.lines.pct).toBe(100);
  expect(s.branches.pct).toBe(100);
});
