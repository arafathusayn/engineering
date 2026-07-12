// Shared helpers: serialize canon data back into the exact JS literals used by
// index.html, and inject them into the template. Used by build.ts, check.ts
// and the extract.ts self-test, so all three agree on the output format.

export interface Card {
  /** name */ n: string;
  /** category (must be one of cats) */ c: string;
  /** subgroup */ s: string;
  /** origin */ o: string;
  /** optional label override for u ("Spot it", "Takeaway", …) */ ul?: string;
  /** optional label override for w ("Escape", "Nuance", …) */ wl?: string;
  /** definition */ d: string;
  /** use when */ u: string;
  /** watch out */ w: string;
  /** sources */ l: [label: string, url: string][];
}

export interface LineageEntry {
  /** year */ y: number;
  /** predecessor card names; "!" prefix = challenges */ p?: string[];
}

/** A presentational line inside the DATA literal, inserted before card index
 * `at`; label "" is a blank line, anything else a "===== label =====" comment banner. */
export interface DataBreak {
  at: number;
  label: string;
}

export interface Layout {
  dataBreaks?: DataBreak[];
  /** entries per physical line of the LINEAGE literal */ lineageRows?: number[];
}

export interface CanonData {
  cats: string[];
  intros: Record<string, string>;
  cards: Card[];
  lineage: Record<string, LineageEntry>;
  layout?: Layout;
}

export const TOKENS = {
  CATS: '"__CANON_CATS__"',
  INTROS: '"__CANON_INTROS__"',
  DATA: '"__CANON_DATA__"',
  LINEAGE: '"__CANON_LINEAGE__"',
} as const;

const J = JSON.stringify;

// A closing script tag inside a JS string literal would terminate the inline
// <script> early; "<\/script" is the same string to JS but inert to the HTML
// parser. Current data never triggers this, so bytes are normally untouched.
function guardScript(js: string): string {
  return js.replace(/<\/(script)/gi, "<\\/$1");
}

export function renderCats(cats: string[]): string {
  return J(cats, null, 2);
}

export function renderIntros(intros: Record<string, string>): string {
  return J(intros, null, 2);
}

// One card, formatted exactly like the source: header fields on the first
// line, then d / u / w / l each on their own line with a one-space hang.
function renderCard(card: Card): string {
  const head = [`n:${J(card.n)}`, `c:${J(card.c)}`, `s:${J(card.s)}`, `o:${J(card.o)}`];
  if (card.ul !== undefined) head.push(`ul:${J(card.ul)}`);
  if (card.wl !== undefined) head.push(`wl:${J(card.wl)}`);
  return [
    `{${head.join(", ")},`,
    ` d:${J(card.d)},`,
    ` u:${J(card.u)},`,
    ` w:${J(card.w)},`,
    ` l:${J(card.l)}}`,
  ].join("\n");
}

// Unknown/stale layout entries are dropped silently: a break pointing past the
// last card can only come from an edit, and formatting must never block one.
export function renderData(cards: Card[], dataBreaks: DataBreak[] = []): string {
  const byAt = new Map<number, string[]>();
  for (const b of dataBreaks) {
    if (!Number.isInteger(b.at) || b.at < 0 || b.at > cards.length) continue;
    if (!byAt.has(b.at)) byAt.set(b.at, []);
    byAt.get(b.at)!.push(b.label);
  }
  const out: string[] = [];
  for (let i = 0; i <= cards.length; i++) {
    for (const label of byAt.get(i) ?? []) {
      out.push(label === "" ? "" : `/* ================= ${label} ================= */`);
    }
    if (i < cards.length) out.push(renderCard(cards[i]!) + (i === cards.length - 1 ? "" : ","));
  }
  return `[\n${out.join("\n")}\n]`;
}

// The source groups related entries (e.g. the 23 GoF patterns) on shared
// lines; lineageRows reproduces that. Falls back to one entry per line when
// the layout no longer matches the entry count, so edits can't corrupt it.
export function renderLineage(lineage: Record<string, LineageEntry>, lineageRows: number[] = []): string {
  const entries = Object.entries(lineage).map(([name, v]) => {
    const p = v.p !== undefined ? `,p:${J(v.p)}` : "";
    return `${J(name)}:{y:${v.y}${p}}`;
  });
  let rows = lineageRows;
  const valid =
    Array.isArray(rows) &&
    rows.every((r) => Number.isInteger(r) && r > 0) &&
    rows.reduce((a, b) => a + b, 0) === entries.length;
  if (!valid) rows = entries.map(() => 1);
  const lines: string[] = [];
  let i = 0;
  for (const count of rows) {
    lines.push(entries.slice(i, i + count).join(","));
    i += count;
  }
  return `{\n${lines.join(",\n")}\n}`;
}

// Replace each placeholder token in the template with its rendered literal.
// Every token must appear exactly once; replacement avoids String.replace so
// `$`-sequences in the data can never be misinterpreted.
export function buildHtml(template: string, data: CanonData): string {
  const rendered: Record<keyof typeof TOKENS, string> = {
    CATS: renderCats(data.cats),
    INTROS: renderIntros(data.intros),
    DATA: renderData(data.cards, data.layout?.dataBreaks),
    LINEAGE: renderLineage(data.lineage, data.layout?.lineageRows),
  };
  let html = template;
  for (const [name, token] of Object.entries(TOKENS) as [keyof typeof TOKENS, string][]) {
    const parts = html.split(token);
    if (parts.length !== 2) {
      throw new Error(`template: expected exactly one ${token}, found ${parts.length - 1}`);
    }
    html = parts[0] + guardScript(rendered[name]) + parts[1];
  }
  return html;
}

// Structural validation of decoded canon data; returns warnings for soft
// referential issues, throws on anything the build can't work with. Checks
// run against the raw runtime shape — the CanonData type is only a claim
// until this function has passed.
export function validate(data: CanonData): string[] {
  const fail = (msg: string): never => {
    throw new Error(`canon.toon: ${msg}`);
  };
  const warnings: string[] = [];
  if (!Array.isArray(data.cats) || data.cats.length === 0) fail("cats must be a non-empty array");
  if (data.cats.some((c) => typeof c !== "string")) fail("cats entries must be strings");
  if (typeof data.intros !== "object" || data.intros === null) fail("intros must be an object");
  if (!Array.isArray(data.cards) || data.cards.length === 0) fail("cards must be a non-empty array");
  if (typeof data.lineage !== "object" || data.lineage === null) fail("lineage must be an object");

  const names = new Set<string>();
  data.cards.forEach((card, i) => {
    const where = `cards[${i}]${card && card.n ? ` (${card.n})` : ""}`;
    for (const f of ["n", "c", "s", "o", "d", "u", "w"] as const) {
      if (typeof card[f] !== "string") fail(`${where}: field "${f}" must be a string`);
    }
    for (const f of ["ul", "wl"] as const) {
      if (card[f] !== undefined && typeof card[f] !== "string") fail(`${where}: field "${f}" must be a string`);
    }
    if (!Array.isArray(card.l) || card.l.some((x) => !Array.isArray(x) || x.length !== 2 || x.some((s) => typeof s !== "string"))) {
      fail(`${where}: field "l" must be an array of [label, url] string pairs`);
    }
    if (names.has(card.n)) warnings.push(`duplicate card name: ${card.n}`);
    names.add(card.n);
    if (!data.cats.includes(card.c)) warnings.push(`${where}: category "${card.c}" not in cats`);
  });
  for (const key of Object.keys(data.intros)) {
    if (!data.cats.includes(key)) warnings.push(`intros key "${key}" not in cats`);
  }
  for (const [name, v] of Object.entries(data.lineage)) {
    if (!Number.isInteger(v.y)) fail(`lineage "${name}": y must be an integer year`);
    if (v.p !== undefined && (!Array.isArray(v.p) || v.p.some((s) => typeof s !== "string"))) {
      fail(`lineage "${name}": p must be an array of strings`);
    }
    if (!names.has(name)) warnings.push(`lineage key "${name}" has no matching card`);
    for (const pred of v.p ?? []) {
      if (!names.has(pred.replace(/^!/, ""))) warnings.push(`lineage "${name}": predecessor "${pred}" has no matching card`);
    }
  }
  return warnings;
}
