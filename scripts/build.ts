// Build index.html from canon.toon (structured data) + template.html
// (everything else). This is the only supported way to change index.html:
// edit canon.toon, run `bun run build`.

import fs from "node:fs";
import path from "node:path";
import { decode } from "@toon-format/toon";
import { buildHtml, validate, type CanonData } from "./lib.ts";

const root = path.join(import.meta.dir, "..");

const data = decode(fs.readFileSync(path.join(root, "canon.toon"), "utf8")) as unknown as CanonData;
const template = fs.readFileSync(path.join(root, "template.html"), "utf8");

for (const warning of validate(data)) console.warn(`warning: ${warning}`);

const html = buildHtml(template, data);
const out = path.join(root, "index.html");
const changed = !fs.existsSync(out) || fs.readFileSync(out, "utf8") !== html;
fs.writeFileSync(out, html);

console.log(
  `built index.html (${Buffer.byteLength(html).toLocaleString()} bytes) from ` +
    `${data.cards.length} cards, ${data.cats.length} categories, ` +
    `${Object.keys(data.lineage).length} lineage entries` +
    (changed ? "" : " — no changes")
);
