# Arafat's Engineering Canon

A single-page reference of software-engineering principles, practices and
patterns. The page you deploy is `index.html`; it is **built**, not edited.

## How the pieces fit

| File | Role |
| --- | --- |
| `canon.toon` | **Source of truth for all content** — categories, intros, the 208 cards, and the lineage DAG, in [TOON](https://toonformat.dev/) format. |
| `template.html` | Everything else — markup, CSS, and the runtime script — with four placeholder tokens (`"__CANON_CATS__"`, `"__CANON_INTROS__"`, `"__CANON_DATA__"`, `"__CANON_LINEAGE__"`) where the data literals go. |
| `index.html` | Build artifact, kept committed so the page can be served as-is. Regenerate with `npm run build`; never edit by hand. |
| `scripts/` | The build system (`build.ts`, `check.ts`, `extract.ts`, shared `lib.ts`), TypeScript running on [Bun](https://bun.sh). |

## Commands

Requires Bun ≥ 1.3 (runs the TypeScript directly, no transpile step).

```sh
bun install        # once; installs @toon-format/toon + @toon-format/cli + tsc
bun run build      # canon.toon + template.html -> index.html
bun run check      # verifies index.html matches the build output byte-for-byte
                   # and that the generated inline script parses as valid JS
bun run extract    # inverse of build: re-derives canon.toon + template.html
                   # from index.html (only needed if index.html was hand-edited)
bun run typecheck  # tsc --noEmit over scripts/
```

The build is deterministic and reproduces the committed `index.html`
byte-for-byte; `npm run check` fails if the three files ever drift apart.

## Editing content

1. Edit `canon.toon` — e.g. add a card under `cards[208]:` (bump the count),
   or a lineage entry under `lineage:`. A `!` prefix on a predecessor in `p`
   means "challenges" rather than "builds on".
2. Run `bun run build` (it validates the data and warns about dangling
   references, e.g. a lineage key with no matching card).
3. Commit both `canon.toon` and the regenerated `index.html`.

You can inspect or convert the data with the official TOON CLI, e.g.
`bunx toon --decode canon.toon -o canon.json` (see the
[TOON CLI docs](https://toonformat.dev/cli/)).

### Data model (`canon.toon`)

- `cats` — ordered category list; `intros` — one intro paragraph per category.
- `cards` — one object per card: `n` name, `c` category, `s` subgroup,
  `o` origin, `d` definition, `u` use-when, `w` watch-out, optional
  `ul`/`wl` label overrides, `l` list of `[sourceLabel, url]` pairs.
- `lineage` — `{cardName: {y: year, p: [predecessors]}}` forming a DAG.
- `layout` — purely presentational hints so the build can reproduce the
  original formatting of `index.html` exactly: `dataBreaks` places the
  `/* ===== SECTION ===== */` banners and blank lines between cards
  (`at` = card index the line precedes, empty label = blank line), and
  `lineageRows` records how many lineage entries share each physical line.
  If a layout hint no longer matches the data after an edit, the build
  falls back to a plain one-entry-per-line rendering — it never breaks.

Changes to presentation (markup, CSS, behavior) go in `template.html`.
