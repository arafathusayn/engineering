// Extract the structured data (CATS / INTROS / DATA / LINEAGE) out of
// index.html into canon.toon, and everything else into template.html with
// placeholder tokens. Re-runnable: if index.html is ever edited by hand,
// `bun run extract` re-derives both files, then verifies that rebuilding
// from them reproduces index.html byte-for-byte.

import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import { encode, decode } from "@toon-format/toon";
import { TOKENS, buildHtml, type CanonData, type Card, type DataBreak, type LineageEntry } from "./lib.ts";

const root = path.join(import.meta.dir, "..");
const html = fs.readFileSync(path.join(root, "index.html"), "utf8");

interface Span {
  openIdx: number;
  endIdx: number;
  text: string;
}

// Locate `const NAME = <literal>;` and return the literal's exact source span.
function findSpan(name: string, open: string, close: string): Span {
  const anchor = `const ${name} = ${open}`;
  const start = html.indexOf(anchor);
  if (start === -1) throw new Error(`index.html: "${anchor}" not found`);
  if (html.indexOf(anchor, start + 1) !== -1) throw new Error(`index.html: "${anchor}" is not unique`);
  const openIdx = start + `const ${name} = `.length;
  const closeIdx = html.indexOf(`\n${close};`, openIdx);
  if (closeIdx === -1) throw new Error(`index.html: terminator for ${name} not found`);
  return { openIdx, endIdx: closeIdx + 2, text: html.slice(openIdx, closeIdx + 2) };
}

const spans: Record<keyof typeof TOKENS, Span> = {
  CATS: findSpan("CATS", "[", "]"),
  INTROS: findSpan("INTROS", "{", "}"),
  DATA: findSpan("DATA", "[", "]"),
  LINEAGE: findSpan("LINEAGE", "{", "}"),
};

// Evaluate each literal in an empty sandbox (they are pure data, no code).
// The JSON round-trip pulls the result into this realm: vm objects carry the
// sandbox's Object.prototype, which the TOON encoder rejects as non-plain.
const evalLiteral = <T>(text: string): T =>
  JSON.parse(JSON.stringify(vm.runInNewContext(`(${text})`, Object.create(null), { timeout: 5000 })));

const cats = evalLiteral<string[]>(spans.CATS.text);
const intros = evalLiteral<Record<string, string>>(spans.INTROS.text);
const cards = evalLiteral<Card[]>(spans.DATA.text);
const lineage = evalLiteral<Record<string, LineageEntry>>(spans.LINEAGE.text);

// Presentational layout inside the DATA literal: blank lines and
// /* ===== SECTION ===== */ banners, recorded against the card index they precede.
const dataBreaks: DataBreak[] = [];
{
  const lines = spans.DATA.text.split("\n");
  let cardIndex = 0;
  for (const line of lines.slice(1, -1)) {
    if (line.startsWith("{n:")) cardIndex++;
    else if (line === "") dataBreaks.push({ at: cardIndex, label: "" });
    else if (/^\/\* =+ .+ =+ \*\/$/.test(line)) {
      dataBreaks.push({ at: cardIndex, label: line.match(/^\/\* =+ (.+?) =+ \*\/$/)![1]! });
    } else if (!/^ [duwl]:/.test(line)) {
      throw new Error(`DATA literal: unrecognized line: ${line.slice(0, 80)}`);
    }
  }
  if (cardIndex !== cards.length) throw new Error("DATA literal: card line count mismatch");
}

// Presentational layout of the LINEAGE literal: entries per physical line.
const lineageRows = spans.LINEAGE.text
  .split("\n")
  .slice(1, -1)
  .map((line) => (line.match(/"(?:[^"\\]|\\.)*":\{/g) ?? []).length);
if (lineageRows.reduce((a, b) => a + b, 0) !== Object.keys(lineage).length) {
  throw new Error("LINEAGE literal: row layout does not sum to entry count");
}

const data: CanonData = { cats, intros, cards, lineage, layout: { dataBreaks, lineageRows } };

// Encode to TOON and prove the round trip is lossless before writing anything.
const toon = encode(data);
if (JSON.stringify(decode(toon)) !== JSON.stringify(data)) {
  throw new Error("TOON round-trip is not lossless; refusing to write canon.toon");
}

// Template = index.html with each literal replaced by its placeholder token
// (replaced back-to-front so earlier offsets stay valid).
let template = html;
for (const name of ["LINEAGE", "DATA", "INTROS", "CATS"] as const) {
  const { openIdx, endIdx } = spans[name];
  template = template.slice(0, openIdx) + TOKENS[name] + template.slice(endIdx);
}

// Self-test: rebuilding from what we are about to write must reproduce
// index.html exactly.
const rebuilt = buildHtml(template, decode(toon) as unknown as CanonData);
if (rebuilt !== html) {
  let i = 0;
  while (rebuilt[i] === html[i]) i++;
  throw new Error(
    `rebuild is not byte-identical; first difference at offset ${i}:\n` +
      `  original: ${JSON.stringify(html.slice(i, i + 80))}\n` +
      `  rebuilt:  ${JSON.stringify(rebuilt.slice(i, i + 80))}`
  );
}

fs.writeFileSync(path.join(root, "canon.toon"), toon);
fs.writeFileSync(path.join(root, "template.html"), template);
console.log(
  `extracted ${cards.length} cards, ${cats.length} categories, ` +
    `${Object.keys(lineage).length} lineage entries -> canon.toon (${Buffer.byteLength(toon).toLocaleString()} bytes)`
);
console.log("template.html written; rebuild verified byte-identical to index.html");
