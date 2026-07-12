// Verify that the committed index.html is exactly what canon.toon +
// template.html build to, and that the generated inline script is valid JS.
// Exits non-zero on any mismatch, so it can gate CI or a pre-commit hook.

import fs from "node:fs";
import path from "node:path";
import { decode } from "@toon-format/toon";
import { buildHtml, validate, type CanonData } from "./lib.ts";

const root = path.join(import.meta.dir, "..");

const data = decode(fs.readFileSync(path.join(root, "canon.toon"), "utf8")) as unknown as CanonData;
const template = fs.readFileSync(path.join(root, "template.html"), "utf8");
const committed = fs.readFileSync(path.join(root, "index.html"), "utf8");

let failed = false;

for (const warning of validate(data)) console.warn(`warning: ${warning}`);

// 1. Byte-for-byte: build output must equal the committed index.html.
const built = buildHtml(template, data);
if (built === committed) {
  console.log("ok: index.html matches the canon.toon + template.html build output");
} else {
  failed = true;
  let i = 0;
  while (built[i] === committed[i]) i++;
  const line = committed.slice(0, i).split("\n").length;
  console.error(`FAIL: index.html differs from build output at offset ${i} (line ${line})`);
  console.error(`  committed: ${JSON.stringify(committed.slice(i, i + 80))}`);
  console.error(`  built:     ${JSON.stringify(built.slice(i, i + 80))}`);
  console.error("  run `bun run build` to regenerate index.html from canon.toon");
}

// 2. The inline <script> of the build output must be syntactically valid JS.
// Bun.Transpiler is Bun's real parser; transformSync throws on syntax errors
// (unlike Bun's vm.Script, which defers compilation past construction).
// Anchored to whole lines: the PARSE GUIDE comment mentions "<script>" in
// prose, which an unanchored match would latch onto.
const scripts = [...built.matchAll(/^<script>\n([\s\S]*?)^<\/script>$/gm)].map((m) => m[1]!);
if (scripts.length === 0) {
  failed = true;
  console.error("FAIL: no inline <script> found in build output");
}
for (const src of scripts) {
  try {
    new Bun.Transpiler({ loader: "js" }).transformSync(src);
  } catch (err) {
    failed = true;
    console.error(`FAIL: generated inline script is not valid JS: ${(err as Error).message}`);
  }
}
if (!failed) console.log(`ok: ${scripts.length} inline script(s) parse as valid JS`);

process.exit(failed ? 1 : 0);
