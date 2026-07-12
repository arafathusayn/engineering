//! The canon.toon input model: what content authors write.
//!
//! Field names mirror the TOON wire format (`n`, `c`, `s`, …) — terse on
//! purpose, documented here once; the view layer translates them to full
//! names. `deny_unknown_fields` everywhere: a typo in canon.toon is a build
//! error, never silently dropped content.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;

use crate::text::slugify;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Canon {
    /// Ordered category list; drives section and chip order.
    pub cats: Vec<String>,
    /// One intro paragraph per category.
    pub intros: HashMap<String, String>,
    /// The cards, in page order.
    pub cards: Vec<Card>,
    /// Lineage DAG: card name → year + predecessors.
    pub lineage: HashMap<String, Lineage>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Card {
    /// Name.
    pub n: String,
    /// Category (must be one of `cats`).
    pub c: String,
    /// Subgroup heading; consecutive cards sharing one form a grid.
    pub s: String,
    /// Origin line (author · work · year).
    pub o: String,
    /// Optional label override for `u` ("Spot it", "Takeaway", …).
    #[serde(default)]
    pub ul: Option<String>,
    /// Optional label override for `w` ("Escape", "Nuance", …).
    #[serde(default)]
    pub wl: Option<String>,
    /// Definition.
    pub d: String,
    /// Use-when guidance.
    pub u: String,
    /// Watch-out guidance.
    pub w: String,
    /// Sources: `[label, url]` pairs.
    pub l: Vec<(String, String)>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    /// Year; 0 or absence from `lineage` means undated.
    pub y: i64,
    /// Predecessor card names; a `!` prefix means "challenges".
    #[serde(default)]
    pub p: Option<Vec<String>>,
}

impl Card {
    /// Effective use-when label (`||`-style: empty override falls back).
    pub fn use_label(&self) -> &str {
        self.ul
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Use when")
    }

    /// Effective watch-out label.
    pub fn watch_label(&self) -> &str {
        self.wl
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("Watch out")
    }
}

pub fn load(path: &Path) -> Result<Canon> {
    let toon =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    toon_format::decode_strict(&toon).map_err(|e| anyhow!("cannot decode {}: {e}", path.display()))
}

/// Structural validation. Anything that would render a wrong or ambiguous
/// page is a hard error; soft referential issues come back as warnings.
pub fn validate(canon: &Canon) -> Result<Vec<String>> {
    let mut warnings = Vec::new();

    if canon.cats.is_empty() {
        bail!("cats must not be empty");
    }
    let mut seen_cats = std::collections::HashSet::new();
    for cat in &canon.cats {
        if cat.is_empty() {
            bail!("cats contains an empty category name");
        }
        if cat == "All" {
            bail!("category name \"All\" is reserved for the filter UI");
        }
        if !seen_cats.insert(cat) {
            bail!("duplicate category \"{cat}\" in cats");
        }
    }

    let mut seen_slugs: HashMap<String, &str> = HashMap::new();
    for card in &canon.cards {
        let slug = slugify(&card.n);
        if slug.is_empty() {
            bail!("card \"{}\" produces an empty slug", card.n);
        }
        if let Some(other) = seen_slugs.insert(slug.clone(), &card.n) {
            bail!(
                "cards \"{other}\" and \"{}\" collide on slug \"{slug}\"",
                card.n
            );
        }
        if !canon.cats.contains(&card.c) {
            bail!("card \"{}\" has unknown category \"{}\"", card.n, card.c);
        }
    }

    for key in canon.intros.keys() {
        if !canon.cats.contains(key) {
            warnings.push(format!("intros key \"{key}\" not in cats"));
        }
    }
    for cat in &canon.cats {
        if !canon.intros.contains_key(cat) {
            warnings.push(format!("category \"{cat}\" has no intro"));
        }
    }

    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(n: &str, c: &str) -> Card {
        Card {
            n: n.into(),
            c: c.into(),
            s: "S".into(),
            o: "O".into(),
            ul: None,
            wl: None,
            d: "D".into(),
            u: "U".into(),
            w: "W".into(),
            l: vec![],
        }
    }

    fn canon(cats: &[&str], cards: Vec<Card>) -> Canon {
        Canon {
            cats: cats.iter().map(|s| s.to_string()).collect(),
            intros: cats
                .iter()
                .map(|s| (s.to_string(), format!("{s} intro")))
                .collect(),
            cards,
            lineage: HashMap::new(),
        }
    }

    #[test]
    fn accepts_well_formed_data() {
        let c = canon(&["A"], vec![card("One", "A")]);
        assert!(validate(&c).unwrap().is_empty());
    }

    #[test]
    fn rejects_reserved_and_duplicate_categories() {
        assert!(validate(&canon(&["All"], vec![])).is_err());
        assert!(validate(&canon(&["A", "A"], vec![])).is_err());
        assert!(validate(&canon(&[""], vec![])).is_err());
    }

    #[test]
    fn rejects_slug_collisions_and_unknown_categories() {
        // Same name modulo punctuation → same slug.
        let c = canon(&["A"], vec![card("Foo Bar", "A"), card("Foo, Bar!", "A")]);
        assert!(validate(&c).unwrap_err().to_string().contains("collide"));
        let c = canon(&["A"], vec![card("One", "B")]);
        assert!(
            validate(&c)
                .unwrap_err()
                .to_string()
                .contains("unknown category")
        );
    }

    #[test]
    fn warns_on_intro_mismatches() {
        let mut c = canon(&["A"], vec![card("One", "A")]);
        c.intros.insert("Ghost".into(), "x".into());
        c.intros.remove("A");
        let warnings = validate(&c).unwrap();
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.contains("Ghost")));
        assert!(warnings.iter().any(|w| w.contains("no intro")));
    }

    #[test]
    fn label_overrides_fall_back_when_empty() {
        let mut one = card("One", "A");
        assert_eq!(one.use_label(), "Use when");
        one.ul = Some(String::new());
        assert_eq!(one.use_label(), "Use when");
        one.ul = Some("Spot it".into());
        assert_eq!(one.use_label(), "Spot it");
        assert_eq!(one.watch_label(), "Watch out");
    }
}
