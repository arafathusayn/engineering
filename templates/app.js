"use strict";

/* Progressive enhancement over a fully pre-rendered page. Every card, chip,
   section and chrono row already exists in the HTML (built from canon.toon
   by the Rust generator); this script only filters, navigates and draws the
   chrono lineage curves. With JavaScript disabled the whole canon is still
   readable and every cross-reference link still works as a plain anchor. */

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
const emptyEl = document.getElementById("empty");
const chipsEl = document.getElementById("chips");
const qEl = document.getElementById("q");
const countLine = document.getElementById("countLine");

const CARDS = Array.from(listView.querySelectorAll("article.card")).map(node => ({
  el: node,
  id: node.id.slice("card-".length),
  cat: node.dataset.cat,
  hay: node.dataset.search
}));
const BYID = new Map(CARDS.map(c => [c.id, c]));
const TOTAL = CARDS.length;
const NCATS = chipsEl.querySelectorAll(".chip").length - 1;
const ROWS = Array.from(chronoView.querySelectorAll(".crow"));
const SECTIONS = Array.from(listView.querySelectorAll(".section"));
const DECADES = Array.from(chronoView.querySelectorAll(".cdecade"));

let activeCat = "All";
const VIEW = { mode: "list" };
let selId = null;

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
  const { vis, q } = visibleIds();
  if (VIEW.mode === "chrono") renderChrono(vis); else renderList(vis, q);
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
  countLine.textContent = shown === TOTAL
    ? TOTAL + " entries · " + NCATS + " categories"
    : shown + " of " + TOTAL + " entries";
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
    if (VIEW.mode !== "chrono") return; // view flipped before this frame
    const wrap = chronoView.querySelector(".chrono");
    const n = drawEdges(wrap);
    countLine.textContent = shown + " of " + TOTAL + " entries · " + n + " lineage links in view";
  });
}

/* ---------- chrono lineage curves (geometry depends on layout, so JS-only) ---------- */
function drawEdges(container) {
  const rows = new Map();
  container.querySelectorAll(".crow[data-id]:not([hidden])").forEach(r => rows.set(r.dataset.id, r));
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
  container.querySelectorAll(".crow").forEach(r => {
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
  const c = chronoView.querySelector(".chrono");
  if (c) applySel(c);
}

/* ---------- category chips ---------- */
function setCat(cat, btn) {
  activeCat = cat;
  for (const c of chipsEl.children) c.setAttribute("aria-pressed", "false");
  btn.setAttribute("aria-pressed", "true");
  render();
  window.scrollTo({ top: 0, behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth" });
}
chipsEl.addEventListener("click", (e) => {
  const b = e.target.closest("button.chip");
  if (b && b.dataset.cat) setCat(b.dataset.cat, b);
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
const viewSeg = document.getElementById("viewSeg");

function setView(v) {
  VIEW.mode = v;
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
let __pulseT, __pulseEl;
function gotoCard(id) {
  selId = null;
  if (VIEW.mode !== "list") setView("list");
  const it = BYID.get(id);
  if (!it) return;
  if (activeCat !== "All" && it.cat !== activeCat) {
    activeCat = "All";
    for (const c of chipsEl.children) c.setAttribute("aria-pressed", String(c === chipsEl.firstElementChild));
  }
  if (qEl.value) qEl.value = "";
  render();
  const elc = document.getElementById("card-" + id);
  if (elc) {
    /* Cards persist across renders now, so clear any previous pulse — a
       stale timer would otherwise cut a re-triggered pulse short. */
    if (__pulseEl) { clearTimeout(__pulseT); __pulseEl.classList.remove("pulse"); }
    __pulseEl = elc;
    elc.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "center" });
    elc.classList.add("pulse");
    __pulseT = setTimeout(() => { elc.classList.remove("pulse"); if (__pulseEl === elc) __pulseEl = null; }, 1800);
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
    const c = chronoView.querySelector(".chrono");
    if (c) applySel(c);
  }
});
let __rzT;
window.addEventListener("resize", () => {
  if (VIEW.mode !== "chrono") return;
  clearTimeout(__rzT);
  __rzT = setTimeout(() => { const c = chronoView.querySelector(".chrono"); if (c) drawEdges(c); }, 150);
});
