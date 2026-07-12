//! Build index.html from canon.toon.
//!
//! Reads the structured data (categories, intros, cards, lineage DAG) from
//! canon.toon, derives everything the page needs (slugs, per-category card
//! groups, per-card lineage cross-references, the decade-grouped chronology),
//! and renders the complete page through the Askama templates in templates/.
//! The output is fully pre-rendered: no client-side JavaScript is needed to
//! display the content.
//!
//! Usage:
//!   cargo run --release            # write index.html
//!   cargo run --release -- --check # verify index.html matches the build

use std::collections::{HashMap, HashSet};
use std::fs;
use std::process::ExitCode;

use askama::Template;
use serde::Deserialize;

// ---------- canon.toon input model ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Canon {
    cats: Vec<String>,
    intros: HashMap<String, String>,
    cards: Vec<CardIn>,
    lineage: HashMap<String, LineageIn>,
    /// Formatting hints used by the earlier JS-literal build; ignored here.
    #[serde(default)]
    #[allow(dead_code)]
    layout: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CardIn {
    n: String,
    c: String,
    s: String,
    o: String,
    #[serde(default)]
    ul: Option<String>,
    #[serde(default)]
    wl: Option<String>,
    d: String,
    u: String,
    w: String,
    l: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LineageIn {
    y: i64,
    #[serde(default)]
    p: Option<Vec<String>>,
}

// ---------- template view model ----------

struct Chip {
    cat: String,
    count: usize,
}

struct LinkV {
    label: String,
    url: String,
}

struct RefV {
    id: String,
    n: String,
}

struct LinGroup {
    label: &'static str,
    x: bool,
    refs: Vec<RefV>,
}

struct CardV {
    id: String,
    n: String,
    c: String,
    o: String,
    d: String,
    u: String,
    w: String,
    ul: String,
    wl: String,
    search: String,
    links: Vec<LinkV>,
    groups: Vec<LinGroup>,
}

struct GroupV {
    subhead: String,
    cards: Vec<CardV>,
}

struct SectionV {
    cat: String,
    intro: String,
    groups: Vec<GroupV>,
}

struct RowV {
    id: String,
    yr: String,
    n: String,
    c: String,
}

struct DecadeV {
    label: String,
    rows: Vec<RowV>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct Page {
    total: usize,
    cats_len: usize,
    chips: Vec<Chip>,
    sections: Vec<SectionV>,
    decades: Vec<DecadeV>,
    edge_json: String,
}

// ---------- derivations ----------

/// Mirror of the page's historical JS slugify, so every #card-<slug> anchor
/// stays stable: lowercase, strip [’'“”"().,:&!?·/], collapse everything
/// outside ASCII [a-z0-9] to single dashes, trim dashes.
fn slugify(name: &str) -> String {
    const STRIP: &str = "\u{2019}'\u{201c}\u{201d}\"().,:&!?\u{00b7}/";
    let mut out = String::new();
    let mut pending_dash = false;
    for ch in name.to_lowercase().chars() {
        if STRIP.contains(ch) {
            continue;
        }
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(ch);
        } else {
            pending_dash = true;
        }
    }
    out
}

/// Deterministic stand-in for the browser's locale-aware localeCompare
/// (which the old client-side renderer used to order same-year rows).
/// Approximates ICU root primary weights per character class — whitespace,
/// then punctuation/symbols, then digits, then letters (each ordered by
/// lowercased code point within its class) — with the raw string as tiebreak.
/// E.g. “select” sorts before DRY because “ (punctuation) outranks letters.
fn collate_key(name: &str) -> (Vec<u32>, String) {
    let primary = name
        .to_lowercase()
        .chars()
        .map(|c| {
            let class: u32 = if c.is_whitespace() {
                1
            } else if c.is_numeric() {
                3
            } else if c.is_alphabetic() {
                4
            } else {
                2
            };
            (class << 24) | (c as u32 & 0xFF_FFFF)
        })
        .collect();
    (primary, name.to_string())
}

fn main() -> ExitCode {
    let check = std::env::args().any(|a| a == "--check");

    let toon = match fs::read_to_string("canon.toon") {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read canon.toon: {e}");
            return ExitCode::FAILURE;
        }
    };
    let canon: Canon = match toon_format::decode_strict(&toon) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot decode canon.toon: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut warnings: Vec<String> = Vec::new();
    let mut misses: Vec<String> = Vec::new();

    // Slugs and name lookup.
    let ids: Vec<String> = canon.cards.iter().map(|c| slugify(&c.n)).collect();
    {
        let mut seen = HashSet::new();
        for (i, id) in ids.iter().enumerate() {
            if id.is_empty() {
                eprintln!(
                    "error: card \"{}\" produces an empty slug",
                    canon.cards[i].n
                );
                return ExitCode::FAILURE;
            }
            if !seen.insert(id.clone()) {
                eprintln!("error: duplicate card slug \"{id}\"");
                return ExitCode::FAILURE;
            }
        }
    }
    let by_name: HashMap<&str, usize> = canon
        .cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.n.as_str(), i))
        .collect();
    if by_name.len() != canon.cards.len() {
        eprintln!("error: duplicate card names");
        return ExitCode::FAILURE;
    }
    {
        let mut seen = HashSet::new();
        for cat in &canon.cats {
            if !seen.insert(cat) {
                eprintln!("error: duplicate category \"{cat}\" in cats");
                return ExitCode::FAILURE;
            }
        }
    }
    for card in &canon.cards {
        if !canon.cats.contains(&card.c) {
            eprintln!(
                "error: card \"{}\" has unknown category \"{}\"",
                card.n, card.c
            );
            return ExitCode::FAILURE;
        }
    }
    for key in canon.intros.keys() {
        if !canon.cats.contains(key) {
            warnings.push(format!("intros key \"{key}\" not in cats"));
        }
    }

    // Orphan lineage keys come first in `misses` (matching the old page) and
    // sorted, so the JSON embedded in the page never depends on hash order.
    let mut orphan_keys: Vec<&String> = canon
        .lineage
        .keys()
        .filter(|k| !by_name.contains_key(k.as_str()))
        .collect();
    orphan_keys.sort();
    misses.extend(orphan_keys.into_iter().map(|k| format!("key:{k}")));

    // Lineage: per-card year + predecessors; edges in card (target) order.
    let mut years: Vec<Option<i64>> = vec![None; canon.cards.len()];
    let mut preds: Vec<Vec<(usize, bool)>> = vec![Vec::new(); canon.cards.len()];
    for (i, card) in canon.cards.iter().enumerate() {
        if let Some(entry) = canon.lineage.get(&card.n) {
            years[i] = Some(entry.y);
            for raw in entry.p.as_deref().unwrap_or(&[]) {
                let (x, name) = match raw.strip_prefix('!') {
                    Some(rest) => (true, rest),
                    None => (false, raw.as_str()),
                };
                match by_name.get(name) {
                    Some(&src) => preds[i].push((src, x)),
                    None => misses.push(format!("{} -> {}", card.n, name)),
                }
            }
        }
    }
    for miss in &misses {
        warnings.push(format!("lineage miss: {miss}"));
    }

    let mut edges: Vec<(usize, usize, bool)> = Vec::new();
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); canon.cards.len()];
    for (t, plist) in preds.iter().enumerate() {
        for &(s, x) in plist {
            outs[s].push(edges.len());
            edges.push((s, t, x));
        }
    }

    // Card views.
    let make_ref = |i: usize| RefV {
        id: ids[i].clone(),
        n: canon.cards[i].n.clone(),
    };
    let card_view = |i: usize| -> CardV {
        let card = &canon.cards[i];
        let mut groups = Vec::new();
        let mut push = |label: &'static str, x: bool, refs: Vec<RefV>| {
            if !refs.is_empty() {
                groups.push(LinGroup { label, x, refs });
            }
        };
        push(
            "Builds on",
            false,
            preds[i]
                .iter()
                .filter(|&&(_, x)| !x)
                .map(|&(s, _)| make_ref(s))
                .collect(),
        );
        push(
            "Challenges",
            true,
            preds[i]
                .iter()
                .filter(|&&(_, x)| x)
                .map(|&(s, _)| make_ref(s))
                .collect(),
        );
        push(
            "Leads to",
            false,
            outs[i]
                .iter()
                .filter(|&&e| !edges[e].2)
                .map(|&e| make_ref(edges[e].1))
                .collect(),
        );
        push(
            "Challenged by",
            true,
            outs[i]
                .iter()
                .filter(|&&e| edges[e].2)
                .map(|&e| make_ref(edges[e].1))
                .collect(),
        );
        CardV {
            id: ids[i].clone(),
            n: card.n.clone(),
            c: card.c.clone(),
            o: card.o.clone(),
            d: card.d.clone(),
            u: card.u.clone(),
            w: card.w.clone(),
            // `||`-style fallback like the old page: empty overrides default too.
            ul: card
                .ul
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Use when".to_string()),
            wl: card
                .wl
                .clone()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Watch out".to_string()),
            search: format!(
                "{} {} {} {} {} {} {}",
                card.n, card.o, card.d, card.u, card.w, card.c, card.s
            )
            .to_lowercase(),
            links: card
                .l
                .iter()
                .map(|(label, url)| LinkV {
                    label: label.clone(),
                    url: url.clone(),
                })
                .collect(),
            groups,
        }
    };

    // List view: categories in cats order, cards in canon order, subgroup runs.
    let mut sections = Vec::new();
    for cat in &canon.cats {
        let items: Vec<usize> = (0..canon.cards.len())
            .filter(|&i| &canon.cards[i].c == cat)
            .collect();
        if items.is_empty() {
            warnings.push(format!("category \"{cat}\" has no cards"));
            continue;
        }
        let mut groups: Vec<GroupV> = Vec::new();
        for &i in &items {
            let sub = &canon.cards[i].s;
            if groups.last().map(|g| &g.subhead) != Some(sub) {
                groups.push(GroupV {
                    subhead: sub.clone(),
                    cards: Vec::new(),
                });
            }
            groups.last_mut().unwrap().cards.push(card_view(i));
        }
        // Non-contiguous repeats of a subgroup would render duplicate
        // subheads; the data keeps runs contiguous, so just warn.
        let mut seen = HashSet::new();
        for g in &groups {
            if !seen.insert(&g.subhead) {
                warnings.push(format!(
                    "category \"{cat}\": subgroup \"{}\" is non-contiguous",
                    g.subhead
                ));
            }
        }
        sections.push(SectionV {
            cat: cat.clone(),
            intro: canon.intros.get(cat).cloned().unwrap_or_default(),
            groups,
        });
    }

    // Chrono view: dated cards by (year, name), grouped by decade; undated
    // last. Year 0 counts as undated, matching the old page's JS truthiness.
    let is_dated = |i: usize| years[i].is_some_and(|y| y != 0);
    let mut dated: Vec<usize> = (0..canon.cards.len()).filter(|&i| is_dated(i)).collect();
    dated.sort_by_key(|&i| (years[i].unwrap(), collate_key(&canon.cards[i].n)));
    let mut undated: Vec<usize> = (0..canon.cards.len()).filter(|&i| !is_dated(i)).collect();
    undated.sort_by_key(|&i| collate_key(&canon.cards[i].n));

    let row_view = |i: usize| -> RowV {
        let card = &canon.cards[i];
        let yr = match years[i] {
            Some(y) if y != 0 && card.o.contains(&y.to_string()) => y.to_string(),
            _ => String::new(),
        };
        RowV {
            id: ids[i].clone(),
            yr,
            n: card.n.clone(),
            c: card.c.clone(),
        }
    };

    let mut decades: Vec<DecadeV> = Vec::new();
    for &i in &dated {
        // div_euclid floors like the old page's Math.floor (matters for BCE years).
        let decade = years[i].unwrap().div_euclid(10) * 10;
        let label = format!("{decade}s");
        if decades.last().map(|d| &d.label) != Some(&label) {
            decades.push(DecadeV {
                label,
                rows: Vec::new(),
            });
        }
        decades.last_mut().unwrap().rows.push(row_view(i));
    }
    if !undated.is_empty() {
        decades.push(DecadeV {
            label: "Undated · folklore & standing practice".to_string(),
            rows: undated.iter().map(|&i| row_view(i)).collect(),
        });
    }

    // Lineage DAG as JSON for the chrono curves / chain highlighting.
    // Escape '<' so the blob can never terminate its <script> element.
    let edge_json = serde_json::json!({
        "edges": edges
            .iter()
            .map(|&(s, t, x)| {
                serde_json::json!([ids[s], ids[t], if x { 1 } else { 0 }])
            })
            .collect::<Vec<_>>(),
        "misses": misses,
    })
    .to_string()
    .replace('<', "\\u003c");

    let chips = canon
        .cats
        .iter()
        .map(|cat| Chip {
            cat: cat.clone(),
            count: canon.cards.iter().filter(|c| &c.c == cat).count(),
        })
        .collect();

    let page = Page {
        total: canon.cards.len(),
        cats_len: canon.cats.len(),
        chips,
        sections,
        decades,
        edge_json,
    };

    for w in &warnings {
        eprintln!("warning: {w}");
    }

    let mut html = match page.render() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: template rendering failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    // POSIX text file: Askama drops the template's final newline.
    if !html.ends_with('\n') {
        html.push('\n');
    }

    if check {
        match fs::read_to_string("index.html") {
            Ok(existing) if existing == html => {
                println!("ok: index.html matches the canon.toon build output");
                ExitCode::SUCCESS
            }
            Ok(existing) => {
                let at = existing
                    .bytes()
                    .zip(html.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| existing.len().min(html.len()));
                eprintln!(
                    "FAIL: index.html differs from build output at byte {at}; run `cargo run --release` to regenerate"
                );
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("FAIL: cannot read index.html: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        if let Err(e) = fs::write("index.html", &html) {
            eprintln!("error: cannot write index.html: {e}");
            return ExitCode::FAILURE;
        }
        println!(
            "built index.html ({} bytes) from {} cards, {} categories, {} lineage entries, {} edges",
            html.len(),
            canon.cards.len(),
            canon.cats.len(),
            canon.lineage.len(),
            edges.len()
        );
        ExitCode::SUCCESS
    }
}
