//! Derivation of the template view model from the canon data: slugs, the
//! lineage graph and its cross-references, category sections with subgroup
//! runs, the decade-grouped chronology, filter chips, and the edge-data JSON.
//! Everything here is pure and unit-tested; rendering is just Askama over
//! the resulting [`Page`].

use std::collections::HashMap;

use anyhow::Result;
use askama::Template;

use crate::model::{Canon, Lineage};
use crate::text::{collate_key, slugify};

/// The one place the `#card-…` anchor namespace is defined: templates
/// consume it as `{{ card_prefix }}`, and app.js reads it back from the
/// rendered page's `data-card-prefix` attribute.
pub const CARD_PREFIX: &str = "card-";

pub struct Chip {
    pub category: String,
    pub count: usize,
}

pub struct Link {
    pub label: String,
    pub url: String,
}

pub struct Reference {
    pub id: String,
    pub name: String,
}

pub struct LineageGroup {
    pub label: &'static str,
    pub challenges: bool,
    pub refs: Vec<Reference>,
}

pub struct CardView {
    pub id: String,
    pub name: String,
    pub category: String,
    pub origin: String,
    pub definition: String,
    pub use_when: String,
    pub watch_out: String,
    pub use_label: String,
    pub watch_label: String,
    pub links: Vec<Link>,
    pub groups: Vec<LineageGroup>,
}

pub struct Group {
    pub subhead: String,
    pub cards: Vec<CardView>,
}

pub struct Section {
    pub category: String,
    pub intro: String,
    pub groups: Vec<Group>,
}

pub struct Row {
    pub id: String,
    pub year_label: String,
    pub name: String,
    pub category: String,
}

pub struct Decade {
    pub label: String,
    pub rows: Vec<Row>,
}

#[derive(Template)]
#[template(path = "index.html")]
pub struct Page {
    pub total: usize,
    pub card_prefix: &'static str,
    pub chips: Vec<Chip>,
    pub sections: Vec<Section>,
    pub decades: Vec<Decade>,
    pub edge_json: String,
    pub script: String,
}

pub struct Derived {
    pub page: Page,
    pub warnings: Vec<String>,
    pub edges: usize,
}

/// One lineage edge, source → target, in the same order the old page built
/// its EDGES array (target card order, then predecessor order).
struct Edge {
    source: usize,
    target: usize,
    challenges: bool,
}

/// Build the full view model. `script` is the page's JavaScript, passed
/// through verbatim (never template-processed) into the inline script tag.
pub fn derive(canon: &Canon, script: String) -> Result<Derived> {
    let ids: Vec<String> = canon.cards.iter().map(|c| slugify(&c.n)).collect();
    let by_name: HashMap<&str, usize> = canon
        .cards
        .iter()
        .enumerate()
        .map(|(i, c)| (c.n.as_str(), i))
        .collect();

    let (years, preds, misses) = lineage_graph(canon, &by_name);
    let warnings: Vec<String> = misses
        .iter()
        .map(|m| format!("lineage miss: {m}"))
        .collect();

    // Edges in old-page order; per-card outgoing lists carry (target, challenges).
    let mut edges: Vec<Edge> = Vec::new();
    let mut outs: Vec<Vec<(usize, bool)>> = vec![Vec::new(); canon.cards.len()];
    for (target, plist) in preds.iter().enumerate() {
        for &(source, challenges) in plist {
            outs[source].push((target, challenges));
            edges.push(Edge {
                source,
                target,
                challenges,
            });
        }
    }

    let cards_by_cat = group_by_category(canon);

    let reference = |i: usize| Reference {
        id: ids[i].clone(),
        name: canon.cards[i].n.clone(),
    };
    // (plain label, challenge label, resolved (card, challenges) pairs).
    type Direction<'a> = (&'static str, &'static str, &'a [(usize, bool)]);
    let card_view = |i: usize| -> CardView {
        let card = &canon.cards[i];
        // The four cross-reference groups, in the old page's display order.
        let directions: [Direction; 2] = [
            ("Builds on", "Challenges", &preds[i]),
            ("Leads to", "Challenged by", &outs[i]),
        ];
        let mut groups = Vec::new();
        for (plain_label, challenge_label, pairs) in directions {
            for (label, challenges) in [(plain_label, false), (challenge_label, true)] {
                let refs: Vec<Reference> = pairs
                    .iter()
                    .filter(|&&(_, x)| x == challenges)
                    .map(|&(other, _)| reference(other))
                    .collect();
                if !refs.is_empty() {
                    groups.push(LineageGroup {
                        label,
                        challenges,
                        refs,
                    });
                }
            }
        }
        CardView {
            id: ids[i].clone(),
            name: card.n.clone(),
            category: card.c.clone(),
            origin: card.o.clone(),
            definition: card.d.clone(),
            use_when: card.u.clone(),
            watch_out: card.w.clone(),
            use_label: card.use_label().to_string(),
            watch_label: card.watch_label().to_string(),
            links: card
                .l
                .iter()
                .map(|(label, url)| Link {
                    label: label.clone(),
                    url: url.clone(),
                })
                .collect(),
            groups,
        }
    };

    let sections = canon
        .cats
        .iter()
        .zip(&cards_by_cat)
        .filter(|(_, items)| !items.is_empty())
        .map(|(cat, items)| Section {
            category: cat.clone(),
            intro: canon.intros.get(cat).cloned().unwrap_or_default(),
            groups: subgroup_runs(canon, items, &card_view),
        })
        .collect();

    let chips = canon
        .cats
        .iter()
        .zip(&cards_by_cat)
        .map(|(cat, items)| Chip {
            category: cat.clone(),
            count: items.len(),
        })
        .collect();

    let row_view = |i: usize| -> Row {
        let card = &canon.cards[i];
        let year_label = match years[i] {
            Some(y) if y != 0 && card.o.contains(&y.to_string()) => y.to_string(),
            _ => String::new(),
        };
        Row {
            id: ids[i].clone(),
            year_label,
            name: card.n.clone(),
            category: card.c.clone(),
        }
    };
    let decades = decades(canon, &years, &row_view);

    let edge_json = edge_json(&edges, &ids, &misses);

    Ok(Derived {
        page: Page {
            total: canon.cards.len(),
            card_prefix: CARD_PREFIX,
            chips,
            sections,
            decades,
            edge_json,
            script: guard_inline_script(script),
        },
        warnings,
        edges: edges.len(),
    })
}

/// Per-card year and resolved predecessors, plus the misses diagnostic in
/// the old page's format: `key:<orphan>` entries first, then
/// `<card> -> <missing predecessor>` (with any `!` prefix stripped).
/// One deliberate divergence from the old page: orphan keys are sorted
/// (the old page used object insertion order, which the TOON decode path
/// does not preserve) so the build stays byte-deterministic.
#[allow(clippy::type_complexity)]
fn lineage_graph(
    canon: &Canon,
    by_name: &HashMap<&str, usize>,
) -> (Vec<Option<i64>>, Vec<Vec<(usize, bool)>>, Vec<String>) {
    let mut orphans: Vec<&String> = canon
        .lineage
        .keys()
        .filter(|k| !by_name.contains_key(k.as_str()))
        .collect();
    orphans.sort();
    let mut misses: Vec<String> = orphans.into_iter().map(|k| format!("key:{k}")).collect();

    let mut years: Vec<Option<i64>> = vec![None; canon.cards.len()];
    let mut preds: Vec<Vec<(usize, bool)>> = vec![Vec::new(); canon.cards.len()];
    for (i, card) in canon.cards.iter().enumerate() {
        let Some(Lineage { y, p }) = canon.lineage.get(&card.n) else {
            continue;
        };
        years[i] = Some(*y);
        for raw in p {
            let (challenges, name) = match raw.strip_prefix('!') {
                Some(rest) => (true, rest),
                None => (false, raw.as_str()),
            };
            match by_name.get(name) {
                Some(&source) => preds[i].push((source, challenges)),
                None => misses.push(format!("{} -> {name}", card.n)),
            }
        }
    }
    (years, preds, misses)
}

/// Card indices per category, in canon order — the one place category
/// membership is computed (validation, sections, and chips all reuse it).
fn group_by_category(canon: &Canon) -> Vec<Vec<usize>> {
    let index_of: HashMap<&str, usize> = canon
        .cats
        .iter()
        .enumerate()
        .map(|(i, c)| (c.as_str(), i))
        .collect();
    let mut grouped: Vec<Vec<usize>> = vec![Vec::new(); canon.cats.len()];
    for (i, card) in canon.cards.iter().enumerate() {
        // Unknown categories are rejected by model::validate before this runs.
        if let Some(&cat) = index_of.get(card.c.as_str()) {
            grouped[cat].push(i);
        }
    }
    grouped
}

/// Consecutive cards sharing a subgroup form one grid under one subhead.
/// model::validate guarantees subgroups are contiguous within a category,
/// so these runs are also the ONLY run each subgroup ever has — which keeps
/// filtered views equivalent to the old renderer's per-filter re-grouping.
fn subgroup_runs(
    canon: &Canon,
    items: &[usize],
    card_view: &impl Fn(usize) -> CardView,
) -> Vec<Group> {
    items
        .chunk_by(|&a, &b| canon.cards[a].s == canon.cards[b].s)
        .map(|run| Group {
            subhead: canon.cards[run[0]].s.clone(),
            cards: run.iter().map(|&i| card_view(i)).collect(),
        })
        .collect()
}

/// Dated cards by (year, collation), split into decade bands (floored, so
/// BCE years band correctly); year 0 and absent both mean undated, last.
fn decades(canon: &Canon, years: &[Option<i64>], row_view: &impl Fn(usize) -> Row) -> Vec<Decade> {
    let decade = |i: usize| years[i].unwrap().div_euclid(10) * 10;
    let (mut dated, mut undated): (Vec<usize>, Vec<usize>) =
        (0..canon.cards.len()).partition(|&i| years[i].is_some_and(|y| y != 0));
    dated.sort_by_cached_key(|&i| (years[i].unwrap(), collate_key(&canon.cards[i].n)));
    undated.sort_by_cached_key(|&i| collate_key(&canon.cards[i].n));

    let mut decades: Vec<Decade> = dated
        .chunk_by(|&a, &b| decade(a) == decade(b))
        .map(|band| Decade {
            label: format!("{}s", decade(band[0])),
            rows: band.iter().map(|&i| row_view(i)).collect(),
        })
        .collect();
    if !undated.is_empty() {
        decades.push(Decade {
            label: "Undated · folklore & standing practice".to_string(),
            rows: undated.iter().map(|&i| row_view(i)).collect(),
        });
    }
    decades
}

/// The lineage DAG as JSON for the chrono curves and chain highlighting.
/// `<` is escaped so the blob can never terminate its `<script>` element.
fn edge_json(edges: &[Edge], ids: &[String], misses: &[String]) -> String {
    serde_json::json!({
        "edges": edges
            .iter()
            .map(|e| serde_json::json!([ids[e.source], ids[e.target], u8::from(e.challenges)]))
            .collect::<Vec<_>>(),
        "misses": misses,
    })
    .to_string()
    .replace('<', "\\u003c")
}

/// Make arbitrary JS safe inside an inline `<script>`: `</script` in any
/// casing would end the element early (the HTML parser is case-insensitive);
/// `<\/script` is the same text to JS. Original casing is preserved.
fn guard_inline_script(script: String) -> String {
    let lower = script.to_ascii_lowercase(); // byte-index compatible
    if !lower.contains("</script") {
        return script;
    }
    let mut out = String::with_capacity(script.len() + 8);
    let mut last = 0;
    for (i, _) in lower.match_indices("</script") {
        out.push_str(&script[last..i]);
        out.push_str("<\\/");
        last = i + 2;
    }
    out.push_str(&script[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fixtures::{card, lineage};

    fn fixture() -> Canon {
        Canon {
            cats: vec!["Alpha".into(), "Beta".into()],
            intros: HashMap::from([("Alpha".into(), "A intro".into())]),
            cards: vec![
                card("One", "Alpha", "First", "Author · 1990"),
                card("Two", "Alpha", "First", "Author · 1994"),
                card("Three", "Alpha", "Second", "Author"),
                card("Four", "Beta", "Only", "Author · 2001"),
            ],
            lineage: HashMap::from([
                ("One".into(), lineage(1990, &[])),
                ("Two".into(), lineage(1994, &["One", "!Ghost"])),
                ("Four".into(), lineage(2001, &["!Two", "One"])),
                ("Orphan Z".into(), lineage(1970, &[])),
                ("Orphan A".into(), lineage(1971, &[])),
            ]),
        }
    }

    fn derived() -> Derived {
        derive(&fixture(), "js".into()).unwrap()
    }

    #[test]
    fn edges_follow_card_then_predecessor_order() {
        let d = derived();
        // Targets in card order (Two, then Four), predecessors in p order.
        let blob: serde_json::Value = serde_json::from_str(&d.page.edge_json).unwrap();
        let edges: Vec<Vec<serde_json::Value>> =
            serde_json::from_value(blob["edges"].clone()).unwrap();
        assert_eq!(
            edges,
            vec![
                vec!["one".into(), "two".into(), serde_json::json!(0)],
                vec!["two".into(), "four".into(), serde_json::json!(1)],
                vec!["one".into(), "four".into(), serde_json::json!(0)],
            ]
        );
    }

    #[test]
    fn misses_are_deterministic_sorted_orphans_then_stripped_edge_misses() {
        let d = derived();
        let blob: serde_json::Value = serde_json::from_str(&d.page.edge_json).unwrap();
        let misses: Vec<String> = serde_json::from_value(blob["misses"].clone()).unwrap();
        // Orphan keys sorted first; the "!" prefix is stripped from edge misses.
        assert_eq!(misses, vec!["key:Orphan A", "key:Orphan Z", "Two -> Ghost"]);
    }

    #[test]
    fn cross_reference_groups_cover_both_directions() {
        let d = derived();
        let one = &d.page.sections[0].groups[0].cards[0];
        assert_eq!(one.name, "One");
        let labels: Vec<&str> = one.groups.iter().map(|g| g.label).collect();
        assert_eq!(labels, vec!["Leads to"]); // One → Two, One → Four (both plain)
        assert_eq!(one.groups[0].refs.len(), 2);
        let four = &d.page.sections[1].groups[0].cards[0];
        let labels: Vec<&str> = four.groups.iter().map(|g| g.label).collect();
        assert_eq!(labels, vec!["Builds on", "Challenges"]);
        assert_eq!(four.groups[1].refs[0].name, "Two");
    }

    #[test]
    fn subgroup_runs_split_only_on_change() {
        let d = derived();
        let alpha = &d.page.sections[0];
        let subs: Vec<&str> = alpha.groups.iter().map(|g| g.subhead.as_str()).collect();
        assert_eq!(subs, vec!["First", "Second"]);
        assert_eq!(alpha.groups[0].cards.len(), 2);
    }

    #[test]
    fn decades_floor_years_and_treat_zero_as_undated() {
        let mut canon = fixture();
        canon
            .cards
            .push(card("Ancient", "Beta", "Only", "Plato · -387"));
        canon.cards.push(card("Timeless", "Beta", "Only", "Nobody"));
        canon.lineage.insert("Ancient".into(), lineage(-387, &[]));
        canon.lineage.insert("Timeless".into(), lineage(0, &[]));
        let d = derive(&canon, String::new()).unwrap();
        let labels: Vec<&str> = d.page.decades.iter().map(|d| d.label.as_str()).collect();
        assert_eq!(
            labels,
            vec![
                "-390s",
                "1990s",
                "2000s",
                "Undated · folklore & standing practice"
            ]
        );
        let undated = d.page.decades.last().unwrap();
        assert!(undated.rows.iter().any(|r| r.name == "Timeless"));
        // Ancient's origin contains "-387"? "Plato · -387" does contain it.
        assert_eq!(d.page.decades[0].rows[0].year_label, "-387");
    }

    #[test]
    fn year_label_shown_only_when_origin_mentions_it() {
        let d = derived();
        let rows: Vec<(String, String)> = d
            .page
            .decades
            .iter()
            .flat_map(|dec| {
                dec.rows
                    .iter()
                    .map(|r| (r.name.clone(), r.year_label.clone()))
            })
            .collect();
        assert!(rows.contains(&("One".into(), "1990".into())));
        assert!(rows.contains(&("Three".into(), String::new()))); // undated
    }

    #[test]
    fn chips_count_every_category_even_when_sections_skip_empties() {
        let mut canon = fixture();
        canon.cats.push("Empty".into());
        let d = derive(&canon, String::new()).unwrap();
        assert_eq!(d.page.chips.len(), 3);
        assert_eq!(d.page.chips[2].count, 0);
        assert_eq!(d.page.sections.len(), 2); // Empty renders no section
    }

    #[test]
    fn inline_script_and_edge_json_cannot_break_out_of_their_tags() {
        let guarded = guard_inline_script("alert('</script><script>')".into());
        assert!(!guarded.contains("</script"));
        assert_eq!(guarded, "alert('<\\/script><script>')");
        // The HTML parser is case-insensitive, so the guard must be too.
        let mixed = guard_inline_script("x = '</ScRiPt>' + '</SCRIPT>'".into());
        assert_eq!(mixed, "x = '<\\/ScRiPt>' + '<\\/SCRIPT>'");
        let d = derive(&fixture(), String::new()).unwrap();
        assert!(!d.page.edge_json.contains('<'));
    }
}
