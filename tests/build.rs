//! Integration: build the real canon.toon end to end and pin the page-level
//! invariants that unit tests can't see.

use std::path::Path;

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn builds_the_real_canon_deterministically() {
    let first = canon_builder::build(root()).expect("build must succeed");
    assert!(
        first.warnings.is_empty(),
        "real data builds clean: {:?}",
        first.warnings
    );

    let again = canon_builder::build(root()).expect("second build must succeed");
    assert_eq!(first.html, again.html, "build must be deterministic");
}

#[test]
fn freshly_compiled_code_matches_its_embedded_source_hash() {
    // The staleness guard for the shipped binary: whatever we just compiled
    // must agree with the checkout it was compiled from.
    canon_builder::verify_freshness(root()).expect("embedded hash matches checkout sources");
}

#[test]
fn page_contains_the_expected_structure() {
    let built = canon_builder::build(root()).unwrap();
    let html = &built.html;

    // Every card pre-rendered, none require JS to exist. Counted via
    // serialization-stable markers: tag openings for cards, and the
    // always-quoted (contains spaces) aria-label prefix for chrono rows.
    assert_eq!(html.matches("<article").count(), built.cards);
    assert_eq!(html.matches(r#"aria-label="Open "#).count(), built.cards);

    // Stable public anchors survive slugification and minification.
    for anchor in [
        "card-single-responsibility-principle",
        "card-di-taxis",
        "card-select-isnt-broken",
    ] {
        assert!(html.contains(anchor), "missing anchor {anchor}");
    }

    // app.js reads the anchor namespace from data-card-prefix; confirm its
    // value is the "card-" prefix the anchors above were generated with,
    // without assuming the minifier quotes the attribute (it currently emits
    // it unquoted).
    let after = html
        .split_once("data-card-prefix=")
        .expect("data-card-prefix present")
        .1;
    let prefix = match after.strip_prefix('"') {
        Some(quoted) => &quoted[..quoted.find('"').expect("closing quote")],
        None => &after[..after.find([' ', '>', '/']).expect("attribute terminator")],
    };
    assert_eq!(prefix, "card-", "data-card-prefix must match the card- anchor namespace");

    // The PARSE GUIDE and the machine-readable lineage blob survive minification.
    assert!(
        html.contains("PARSE GUIDE"),
        "keep_comments must preserve the guide"
    );
    // Locate the edgeData element without assuming how the minifier orders
    // or quotes attributes. Take the LAST candidate: the PARSE GUIDE comment
    // mentions the tag in prose before the real element appears.
    let json = html
        .match_indices("<script")
        .filter_map(|(start, _)| {
            let tag_end = start + html[start..].find('>')?;
            let tag = &html[start..=tag_end];
            (tag.contains("edgeData") && tag.contains("application/json"))
                .then_some(&html[tag_end + 1..])
        })
        .last()
        .expect("edgeData blob present");
    let json = &json[..json.find("</script>").expect("blob closes")];
    let parsed: serde_json::Value =
        serde_json::from_str(json).expect("edgeData must stay valid JSON");
    assert_eq!(parsed["edges"].as_array().unwrap().len(), built.edges);
    assert_eq!(parsed["misses"].as_array().unwrap().len(), 0);

    // Minified but still a POSIX text file.
    assert!(html.ends_with('\n'));
}
