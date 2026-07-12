# Arafat's Engineering Canon

A single-page reference of software-engineering principles, practices and
patterns. The page you deploy is `index.html`; it is **built, not edited** —
and it is fully pre-rendered, so no client-side JavaScript is needed to read
it. The page's script is progressive enhancement only (search, category
filter, the chronological lineage view, theme switching).

## How the pieces fit

| File | Role |
| --- | --- |
| `canon.toon` | **Source of truth for all content** — categories, intros, the 208 cards, and the lineage DAG, in [TOON](https://toonformat.dev/) format. |
| `src/main.rs` | The builder: decodes `canon.toon` with [toon-rust](https://github.com/toon-format/toon-rust), derives slugs / category sections / lineage cross-references / the decade timeline, renders via [Askama](https://askama.readthedocs.io/) templates. |
| `templates/index.html` | The page shell (markup + CSS) with Askama loops where content goes. |
| `templates/card.html` | One card (`<article class="card">`), included per card. |
| `templates/app.js` | The page's only JavaScript: filters and navigates the already-rendered DOM, draws the chrono lineage curves. Never renders content. |
| `index.html` | Build artifact, kept committed so the page can be served as-is. Regenerate with `cargo run --release`; never edit by hand. |

## Commands

Requires Rust ≥ 1.85 (edition 2024).

```sh
cargo run --release            # canon.toon + templates/ -> index.html
cargo run --release -- --check # verify index.html matches the build output
```

The build is deterministic; `--check` fails if `index.html` and `canon.toon`
ever drift apart, so it can gate CI or a pre-commit hook.

## Editing content

1. Edit `canon.toon` — e.g. add a card under `cards[208]:` (bump the count),
   or a lineage entry under `lineage:`. A `!` prefix on a predecessor in `p`
   means "challenges" rather than "builds on".
2. Run `cargo run --release`. The builder validates the data (unknown fields,
   duplicate names/slugs, unknown categories are hard errors; dangling
   lineage references are warnings).
3. Commit both `canon.toon` and the regenerated `index.html`.

Presentation changes (markup, CSS, behavior) go in `templates/`.

You can inspect or convert the data with the official TOON CLI, e.g.
`npx @toon-format/cli --decode canon.toon -o canon.json` (see the
[TOON CLI docs](https://toonformat.dev/cli/)).

### Data model (`canon.toon`)

- `cats` — ordered category list; `intros` — one intro paragraph per category.
- `cards` — one object per card: `n` name, `c` category, `s` subgroup,
  `o` origin, `d` definition, `u` use-when, `w` watch-out, optional
  `ul`/`wl` label overrides, `l` list of `[sourceLabel, url]` pairs.
- `lineage` — `{cardName: {y: year, p: [predecessors]}}` forming a DAG.
  Cards absent from `lineage` render in the timeline's "Undated" group.

### What the builder derives

- Card ids: `slugify(name)` (kept byte-compatible with the page's historical
  JS slugs, so `#card-…` anchors and bookmarks stay stable).
- Per-card cross-references: "Builds on" / "Challenges" from `p`, and the
  reverse "Leads to" / "Challenged by" from the whole graph — rendered as
  plain anchors that work without JavaScript.
- The chrono view: dated cards sorted by year (ICU-like collation within a
  year), grouped by decade, plus the undated group.
- `<script type="application/json" id="edgeData">` — the lineage DAG as data
  for the chrono curves and chain highlighting; `window.__canon` exposes
  `{edges, misses}` at runtime like the page always did.
