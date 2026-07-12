"use strict";

/* Progressive enhancement over a fully pre-rendered page. Every card, chip,
   section and chrono row already exists in the HTML (built from canon.toon
   by the Rust generator); this script only filters, navigates and draws the
   chrono lineage curves. With JavaScript disabled the whole canon is still
   readable and every cross-reference link still works as a plain anchor.
   NOTE: the builder injects this file as opaque data (never parsed as
   template syntax, though the minifier may compress it), so any JS syntax
   is safe here. */

/* ---------- lineage graph (embedded as JSON data, not markup) ---------- */
const EDGE_DATA = JSON.parse(document.getElementById("edgeData").textContent);
const EDGES = EDGE_DATA.edges.map(e => ({ s: e[0], t: e[1], x: !!e[2] }));
window.__canon = { edges: EDGES.map(e => [e.s, e.t]), misses: EDGE_DATA.misses };

const INS = new Map(), OUTS = new Map();
EDGES.forEach((e, i) => {
  if (!INS.has(e.t)) INS.set(e.t, []);
  INS.get(e.t).push(i);
  if (!OUTS.has(e.s)) OUTS.set(e.s, []);
  OUTS.get(e.s).push(i);
});
function chain(id) {
  const lit = new Set([id]), litE = new Set();
  const up = [id];
  while (up.length) {
    const n = up.pop();
    for (const i of (INS.get(n) || [])) {
      if (!litE.has(i)) { litE.add(i); const s = EDGES[i].s; if (!lit.has(s)) { lit.add(s); up.push(s); } }
    }
  }
  const dn = [id];
  while (dn.length) {
    const n = dn.pop();
    for (const i of (OUTS.get(n) || [])) {
      if (!litE.has(i)) { litE.add(i); const t = EDGES[i].t; if (!lit.has(t)) { lit.add(t); dn.push(t); } }
    }
  }
  return { lit, litE };
}

/* ---------- pre-rendered DOM handles ---------- */
const mainEl = document.getElementById("main");
const listView = document.getElementById("listView");
const chronoView = document.getElementById("chronoView");
const chronoWrap = chronoView.querySelector(".chrono");
const emptyEl = document.getElementById("empty");
const chipsEl = document.getElementById("chips");
const qEl = document.getElementById("q");
const countLine = document.getElementById("countLine");
const viewSeg = document.getElementById("viewSeg");

/* The anchor namespace is defined once in the builder (view.rs) and
   published on the page; read it back rather than re-declaring it. */
const CARD_PREFIX = mainEl.dataset.cardPrefix;

/* The search haystack is derived from the same seven fields the page has
   always searched — name, origin, definition, use-when, watch-out, category,
   subgroup, space-joined and lowercased — read back from the pre-rendered
   DOM so the text isn't shipped twice. */
function rowText(p) {
  return p && p.firstElementChild ? p.textContent.slice(p.firstElementChild.textContent.length) : "";
}
function haystack(node, sub) {
  return (
    node.querySelector("h4").textContent + " " +
    node.querySelector(".origin").textContent + " " +
    node.querySelector(".def").textContent + " " +
    rowText(node.querySelector(".row.use")) + " " +
    rowText(node.querySelector(".row.watch")) + " " +
    node.dataset.cat + " " +
    sub
  ).toLowerCase();
}

const CARDS = [];
for (const grid of listView.querySelectorAll(".grid")) {
  const head = grid.previousElementSibling;
  const sub = head && head.classList.contains("subhead") ? head.textContent : "";
  for (const node of grid.querySelectorAll("article.card")) {
    CARDS.push({ el: node, id: node.id.slice(CARD_PREFIX.length), cat: node.dataset.cat, hay: haystack(node, sub) });
  }
}
const BYID = new Map(CARDS.map(c => [c.id, c]));
const TOTAL = CARDS.length;
const ROWS = Array.from(chronoView.querySelectorAll(".crow"));
const SECTIONS = Array.from(listView.querySelectorAll(".section"));
const DECADES = Array.from(chronoView.querySelectorAll(".cdecade"));
const FULL_COUNT = countLine.textContent;

/* ---------- state ---------- */
let activeCat = "All";
let view = "list";
let selId = null;
let pulseTimer, pulsedCard;

function clearPulse() {
  if (pulsedCard) {
    clearTimeout(pulseTimer);
    pulsedCard.classList.remove("pulse");
    pulsedCard = null;
  }
}

/* ---------- filtering ---------- */
function matches(c, q) {
  if (!q) return true;
  return q.split(/\s+/).every(t => c.hay.includes(t));
}

function visibleIds() {
  const q = qEl.value.trim().toLowerCase();
  const vis = new Set();
  for (const c of CARDS) {
    if ((activeCat === "All" || c.cat === activeCat) && matches(c, q)) vis.add(c.id);
  }
  return { vis, q };
}

function render() {
  clearPulse(); // the old full-rebuild implicitly killed the pulse on re-render
  const { vis, q } = visibleIds();
  if (view === "chrono") renderChrono(vis); else renderList(vis, q);
}

function renderList(vis, q) {
  let shown = 0;
  for (const c of CARDS) { const on = vis.has(c.id); c.el.hidden = !on; if (on) shown++; }
  for (const section of SECTIONS) {
    let any = false;
    for (const grid of section.querySelectorAll(".grid")) {
      const on = !!grid.querySelector(".card:not([hidden])");
      grid.hidden = !on;
      const head = grid.previousElementSibling;
      if (head && head.classList.contains("subhead")) head.hidden = !on;
      if (on) any = true;
    }
    section.hidden = !any;
    const intro = section.querySelector(".intro");
    if (intro) intro.hidden = !!q;
  }
  emptyEl.style.display = shown ? "none" : "block";
  countLine.textContent = shown === TOTAL ? FULL_COUNT : shown + " of " + TOTAL + " entries";
}

function renderChrono(vis) {
  let shown = 0;
  for (const r of ROWS) { const on = vis.has(r.dataset.id); r.hidden = !on; if (on) shown++; }
  for (const h of DECADES) {
    let any = false;
    for (let sib = h.nextElementSibling; sib && !sib.classList.contains("cdecade"); sib = sib.nextElementSibling) {
      if (sib.classList.contains("crow") && !sib.hidden) { any = true; break; }
    }
    h.hidden = !any;
  }
  emptyEl.style.display = shown ? "none" : "block";
  requestAnimationFrame(() => {
    if (view !== "chrono") return; // view flipped before this frame
    const n = drawEdges(chronoWrap);
    countLine.textContent = shown + " of " + TOTAL + " entries · " + n + " lineage links in view";
  });
}

/* ---------- chrono lineage curves (geometry depends on layout, so JS-only) ---------- */
function drawEdges(container) {
  const rows = new Map();
  for (const r of ROWS) if (!r.hidden) rows.set(r.dataset.id, r);
  let svg = container.querySelector("svg.cedges");
  if (!svg) {
    svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    svg.setAttribute("class", "cedges");
    container.prepend(svg);
  }
  /* The svg persists across renders (the old page recreated it), and it is
     part of container.scrollHeight — zero it first or its height can only
     ever ratchet upward, leaving dead scroll space after filtering. */
  svg.setAttribute("width", 0);
  svg.setAttribute("height", 0);
  svg.setAttribute("width", container.clientWidth);
  svg.setAttribute("height", container.scrollHeight);
  svg.textContent = "";
  const cRect = container.getBoundingClientRect();
  const pt = (r) => {
    const b = r.querySelector(".dot").getBoundingClientRect();
    return { x: b.left - cRect.left + b.width / 2, y: b.top - cRect.top + b.height / 2 };
  };
  const vis = [];
  EDGES.forEach((e, i) => {
    const s = rows.get(e.s), t = rows.get(e.t);
    if (s && t) vis.push({ e, i, a: pt(s), b: pt(t) });
  });
  for (const o of vis) { o.y1 = Math.min(o.a.y, o.b.y); o.y2 = Math.max(o.a.y, o.b.y); }
  vis.sort((p, q) => (p.y2 - p.y1) - (q.y2 - q.y1));
  const narrow = container.clientWidth < 640;
  const laneBase = narrow ? 12 : 18, laneGap = narrow ? 5 : 9;
  const lanes = [];
  for (const o of vis) {
    let L = 0;
    for (; L < lanes.length; L++) {
      if (!lanes[L].some(iv => o.y1 < iv[1] && iv[0] < o.y2)) break;
    }
    if (L === lanes.length) lanes.push([]);
    if (L > 12) L = 12;
    lanes[L].push([o.y1, o.y2]);
    o.lane = L;
  }
  for (const o of vis) {
    const lx = Math.max(4, o.a.x - (laneBase + o.lane * laneGap));
    const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
    p.setAttribute("d", "M " + (o.a.x - 5) + " " + o.a.y + " C " + lx + " " + o.a.y + ", " + lx + " " + o.b.y + ", " + (o.b.x - 5) + " " + o.b.y);
    p.setAttribute("class", "ce" + (o.e.x ? " x" : ""));
    p.dataset.i = o.i;
    svg.appendChild(p);
  }
  applySel(container);
  return vis.length;
}

function applySel(container) {
  const res = selId ? chain(selId) : null;
  container.classList.toggle("dimmed", !!selId);
  ROWS.forEach(r => {
    const on = res && res.lit.has(r.dataset.id);
    r.classList.toggle("lit", !!on);
    r.classList.toggle("sel", selId === r.dataset.id);
    r.setAttribute("aria-pressed", String(selId === r.dataset.id));
  });
  container.querySelectorAll("path.ce").forEach(p => {
    p.classList.toggle("lit", !!(res && res.litE.has(+p.dataset.i)));
  });
}

function toggleSel(id) {
  selId = (selId === id) ? null : id;
  applySel(chronoWrap);
}

/* ---------- category chips (aria state has exactly one writer) ---------- */
function applyCat(cat) {
  activeCat = cat;
  for (const c of chipsEl.children) c.setAttribute("aria-pressed", String(c.dataset.cat === cat));
}
function setCat(cat) {
  applyCat(cat);
  render();
  window.scrollTo({ top: 0, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
}
chipsEl.addEventListener("click", (e) => {
  const b = e.target.closest("button.chip");
  if (b && b.dataset.cat) setCat(b.dataset.cat);
});

/* ---------- appearance (in-memory only; no storage APIs) ---------- */
const themeBtn = document.getElementById("themeBtn");
const themeMenu = document.getElementById("themeMenu");
const themeItems = Array.from(themeMenu.querySelectorAll("button[data-t]"));
const metaTheme = document.getElementById("metaTheme");
const darkMQ = matchMedia("(prefers-color-scheme: dark)");
let themeMode = "auto";
const themeLabels = { auto: "Auto", light: "Light", dark: "Dark" };

function applyTheme(mode) {
  themeMode = mode;
  if (mode === "auto") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = mode;
  for (const b of themeItems) b.setAttribute("aria-checked", String(b.dataset.t === mode));
  themeBtn.dataset.icon = mode;
  themeBtn.setAttribute("aria-label", "Appearance: " + themeLabels[mode]);
  const dark = mode === "dark" || (mode === "auto" && darkMQ.matches);
  metaTheme.setAttribute("content", dark ? "#000000" : "#f5f5f7");
}

function openThemeMenu() {
  themeMenu.hidden = false;
  themeMenu.classList.add("open");
  themeBtn.setAttribute("aria-expanded", "true");
  (themeItems.find(b => b.getAttribute("aria-checked") === "true") || themeItems[0]).focus();
}
function closeThemeMenu(refocus) {
  themeMenu.classList.remove("open");
  themeMenu.hidden = true;
  themeBtn.setAttribute("aria-expanded", "false");
  if (refocus) themeBtn.focus();
}
themeBtn.addEventListener("click", () => {
  themeMenu.hidden ? openThemeMenu() : closeThemeMenu(true);
});
themeBtn.addEventListener("keydown", (e) => {
  if (e.key === "ArrowDown" && themeMenu.hidden) { e.preventDefault(); openThemeMenu(); }
});
themeMenu.addEventListener("click", (e) => {
  const b = e.target.closest("button[data-t]");
  if (b) { applyTheme(b.dataset.t); closeThemeMenu(true); }
});
themeMenu.addEventListener("keydown", (e) => {
  const i = themeItems.indexOf(document.activeElement);
  if (e.key === "ArrowDown") { e.preventDefault(); themeItems[(i + 1) % themeItems.length].focus(); }
  else if (e.key === "ArrowUp") { e.preventDefault(); themeItems[(i - 1 + themeItems.length) % themeItems.length].focus(); }
  else if (e.key === "Home") { e.preventDefault(); themeItems[0].focus(); }
  else if (e.key === "End") { e.preventDefault(); themeItems[themeItems.length - 1].focus(); }
  else if (e.key === "Escape") { closeThemeMenu(true); }
  else if (e.key === "Tab") { closeThemeMenu(false); }
});
document.addEventListener("pointerdown", (e) => {
  if (!themeMenu.hidden && !e.target.closest("#appearance")) closeThemeMenu(false);
});
darkMQ.addEventListener("change", () => { if (themeMode === "auto") applyTheme("auto"); });
applyTheme("auto");

/* ---------- search ---------- */
qEl.addEventListener("input", render);
document.addEventListener("keydown", (e) => {
  if (e.key === "/" && document.activeElement !== qEl) { e.preventDefault(); qEl.focus(); }
  if (e.key === "Escape" && document.activeElement === qEl) { qEl.value = ""; render(); qEl.blur(); }
});

/* ---------- view toggle ---------- */
function setView(v) {
  view = v;
  selId = null;
  for (const b of viewSeg.querySelectorAll("button")) b.setAttribute("aria-pressed", String(b.dataset.v === v));
  listView.hidden = v !== "list";
  chronoView.hidden = v !== "chrono";
  render();
  window.scrollTo({ top: 0, behavior: "auto" });
}
viewSeg.addEventListener("click", (e) => {
  const b = e.target.closest("button[data-v]");
  if (b) setView(b.dataset.v);
});

/* ---------- cross-reference navigation ---------- */
function gotoCard(id) {
  selId = null;
  if (view !== "list") setView("list");
  const it = BYID.get(id);
  if (!it) return;
  if (activeCat !== "All" && it.cat !== activeCat) applyCat("All");
  if (qEl.value) qEl.value = "";
  render();
  const elc = document.getElementById(CARD_PREFIX + id);
  if (elc) {
    elc.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "center" });
    elc.classList.add("pulse");
    pulsedCard = elc;
    pulseTimer = setTimeout(clearPulse, 1800);
  }
}

mainEl.addEventListener("click", (e) => {
  const a = e.target.closest("a.xref, a.go");
  if (a && a.dataset.id) { e.preventDefault(); gotoCard(a.dataset.id); return; }
  const r = e.target.closest(".crow");
  if (r) toggleSel(r.dataset.id);
});
mainEl.addEventListener("keydown", (e) => {
  if ((e.key === "Enter" || e.key === " ") && e.target.classList && e.target.classList.contains("crow")) {
    e.preventDefault();
    toggleSel(e.target.dataset.id);
  }
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && selId) {
    selId = null;
    applySel(chronoWrap);
  }
});
let resizeTimer;
window.addEventListener("resize", () => {
  if (view !== "chrono") return;
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => { if (view === "chrono") drawEdges(chronoWrap); }, 150);
});

/* Reconcile any search text that existed before this script ran (typed
   during parse, or a browser-restored form value) — the old page did this
   with its unconditional startup render(). */
if (qEl.value) render();
