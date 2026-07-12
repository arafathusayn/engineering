//! Text derivations that must stay stable: card slugs (public anchors) and
//! the collation used to order same-year rows in the chronological view.

use unicode_normalization::UnicodeNormalization;

/// Mirror of the page's historical JS slugify, so every `#card-<slug>` anchor
/// stays stable: lowercase, strip `’'“”"().,:&!?·/`, collapse everything
/// outside ASCII `[a-z0-9]` to single dashes, trim dashes.
pub fn slugify(name: &str) -> String {
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

/// Character class for collation, mirroring ICU root primary-weight order:
/// whitespace < punctuation/symbols < digits < letters.
fn char_class(c: char) -> u8 {
    if c.is_whitespace() {
        1
    } else if c.is_numeric() {
        3
    } else if c.is_alphabetic() {
        4
    } else {
        2
    }
}

/// Deterministic stand-in for the browser's locale-aware `localeCompare`,
/// which the old client-side renderer used to order same-year chrono rows.
/// Approximates ICU primary weights: characters are NFKD-decomposed with
/// combining marks dropped (so `É` collates with `E`), then ordered by
/// (class, lowercased code point); the raw string is the tiebreak.
pub fn collate_key(name: &str) -> (Vec<(u8, char)>, String) {
    let primary = name
        .to_lowercase()
        .nfkd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .map(|c| (char_class(c), c))
        .collect();
    (primary, name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_match_the_historical_js_slugify() {
        // Vectors checked against the old page's slugify() in a JS engine.
        let cases = [
            (
                "Single Responsibility Principle",
                "single-responsibility-principle",
            ),
            (
                "DRY: Don\u{2019}t Repeat Yourself",
                "dry-dont-repeat-yourself",
            ),
            ("Open\u{2013}Closed Principle", "open-closed-principle"),
            (
                "\u{201c}select\u{201d} Isn\u{2019}t Broken",
                "select-isnt-broken",
            ),
            ("Di\u{e1}taxis", "di-taxis"), // á is outside [a-z0-9] and becomes a dash
            (
                "Model\u{2013}View\u{2013}Controller",
                "model-view-controller",
            ),
            ("CQRS", "cqrs"),
            ("NASA\u{2019}s Power of 10", "nasas-power-of-10"),
            ("Gateway, Remote Facade & DTO", "gateway-remote-facade-dto"),
        ];
        for (name, slug) in cases {
            assert_eq!(slugify(name), slug, "slug for {name:?}");
        }
    }

    #[test]
    fn collation_puts_punctuation_before_letters_like_icu() {
        // “select” sorts before DRY because “ (punctuation) outranks letters.
        assert!(collate_key("\u{201c}select\u{201d} Isn\u{2019}t Broken") < collate_key("DRY"));
    }

    #[test]
    fn collation_treats_space_as_lowest_class() {
        assert!(collate_key("design by contract") < collate_key("designer"));
    }

    #[test]
    fn collation_folds_diacritics_onto_base_letters() {
        let eclair = collate_key("\u{c9}clair");
        assert!(
            collate_key("Earlier") < eclair,
            "É collates with E: after 'ea…'"
        );
        assert!(
            eclair < collate_key("Event Sourcing"),
            "…and before 'ev…', not after Z"
        );
        assert!(
            eclair < collate_key("Zulu"),
            "never past the end of the alphabet"
        );
    }

    #[test]
    fn collation_is_case_insensitive_at_primary_strength() {
        assert_eq!(collate_key("KISS").0, collate_key("kiss").0);
    }
}
