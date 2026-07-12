// Test server: serves the REAL built index.html, with the minified inline
// script swapped for the pristine templates/app.js source. The two are
// behaviorally identical (the shipped script is minified from this file),
// and the swap makes V8 coverage map 1:1 onto app.js lines.
//
// Variant pages (query-selected) surgically alter the page so tests can
// reach defensive branches the real content never triggers:
//   /?prefill=<q>  search input carries a value before the script runs
//   /?dense=1      edgeData replaced with >13 mutually overlapping edges
//                  (exercises the chrono lane cap)
//   /?edgecases=1  one use-row without its <b> label, one grid without a
//                  subhead, one section without an intro
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.join(path.dirname(fileURLToPath(import.meta.url)), "..");

export const APP_JS = fs.readFileSync(path.join(ROOT, "templates/app.js"), "utf8");
const PAGE = fs.readFileSync(path.join(ROOT, "index.html"), "utf8");

function replaceOnce(hay, needle, replacement, what) {
  const i = hay.indexOf(needle);
  if (i === -1 || hay.indexOf(needle, i + 1) !== -1) {
    throw new Error(`${what}: expected exactly one occurrence of ${JSON.stringify(needle)}`);
  }
  return hay.slice(0, i) + replacement + hay.slice(i + needle.length);
}

// The app script is the only bare `<script>` element in the minified page.
function withSourceScript(html) {
  const open = html.lastIndexOf("<script>");
  if (open === -1) throw new Error("bare <script> not found");
  const close = html.indexOf("</script>", open);
  return html.slice(0, open) + "<script>" + APP_JS + "</script>" + html.slice(close + "</script>".length);
}

function locateEdgeBlob(html) {
  // Same serialization-agnostic scan as tests/build.rs: last <script …> tag
  // mentioning edgeData (the PARSE GUIDE comment mentions it first, in prose).
  let result = null;
  let from = 0;
  for (;;) {
    const start = html.indexOf("<script", from);
    if (start === -1) break;
    const tagEnd = html.indexOf(">", start);
    const tag = html.slice(start, tagEnd + 1);
    if (tag.includes("edgeData") && tag.includes("application/json")) {
      const close = html.indexOf("</script>", tagEnd);
      result = { contentStart: tagEnd + 1, contentEnd: close };
    }
    from = tagEnd + 1;
  }
  if (!result) throw new Error("edgeData blob not found");
  return result;
}

function withDenseEdges(html) {
  // >13 mutually overlapping lineage edges between far-apart chrono rows.
  const ids = [...html.matchAll(/class=crow data-id=([a-z0-9-]+)/g)].map((m) => m[1]);
  if (ids.length < 40) throw new Error(`only ${ids.length} crow ids found`);
  const edges = [];
  for (let i = 0; i < 16; i++) edges.push([ids[i], ids[i + 20], i % 2]);
  const { contentStart, contentEnd } = locateEdgeBlob(html);
  return (
    html.slice(0, contentStart) + JSON.stringify({ edges, misses: [] }) + html.slice(contentEnd)
  );
}

function withEdgeCases(html) {
  // (a) strip the <b> label from one card's use-row -> rowText() fallback.
  const useRow = /<p class="row use"><b>[^<]*<\/b>/;
  const m = html.match(useRow);
  if (!m) throw new Error("no use-row with label found");
  html = html.replace(useRow, '<p class="row use">');
  // (b) drop the first subhead -> a grid whose previous sibling is the intro.
  const subhead = html.match(/<h3 class=subhead>[^<]*<\/h3>/);
  if (!subhead) throw new Error("no subhead found");
  html = replaceOnce(html, subhead[0], "", "first subhead");
  // (c) drop the first intro -> a section without one. The minifier omits
  // the optional </p>, so the element is the tag plus its text run.
  const intro = html.match(/<p class=intro>[^<]*/);
  if (!intro) throw new Error("no intro found");
  html = replaceOnce(html, intro[0], "", "first intro");
  return html;
}

function withPrefill(html, value) {
  const input = html.match(/<input[^>]*id=q[^>]*>/);
  if (!input) throw new Error("search input not found");
  const patched = input[0].replace(/>$/, ` value="${value}">`);
  return replaceOnce(html, input[0], patched, "search input");
}

export function startServer() {
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, "http://localhost");
    if (url.pathname !== "/") {
      res.writeHead(404).end();
      return;
    }
    let html = withSourceScript(PAGE);
    if (url.searchParams.get("dense")) html = withDenseEdges(html);
    if (url.searchParams.get("edgecases")) html = withEdgeCases(html);
    const prefill = url.searchParams.get("prefill");
    if (prefill) html = withPrefill(html, prefill);
    res.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(html);
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, origin: `http://127.0.0.1:${server.address().port}` });
    });
  });
}
