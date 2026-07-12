//! Integration: build the real canon.toon end to end and pin the page-level
//! invariants that unit tests can't see.

#[test]
fn builds_the_real_canon_deterministically() {
    let first = canon_builder::build().expect("build must succeed");
    assert!(
        first.warnings.is_empty(),
        "real data builds clean: {:?}",
        first.warnings
    );

    let again = canon_builder::build().expect("second build must succeed");
    assert_eq!(first.html, again.html, "build must be deterministic");
}

#[test]
fn page_contains_the_expected_structure() {
    let built = canon_builder::build().unwrap();
    let html = &built.html;

    // Every card pre-rendered, none require JS to exist. (The minifier may
    // reorder attributes, so count tag openings, not attribute sequences.)
    assert_eq!(html.matches("<article").count(), built.cards);
    assert_eq!(html.matches("class=crow").count(), built.cards);

    // Stable public anchors survive slugification and minification.
    for anchor in [
        "card-single-responsibility-principle",
        "card-di-taxis",
        "card-select-isnt-broken",
    ] {
        assert!(html.contains(anchor), "missing anchor {anchor}");
    }

    // The PARSE GUIDE and the machine-readable lineage blob survive minification.
    assert!(
        html.contains("PARSE GUIDE"),
        "keep_comments must preserve the guide"
    );
    // The minifier alphabetizes and unquotes the attributes; rfind because
    // the PARSE GUIDE comment mentions the (unminified) tag in prose first.
    let blob_start = html
        .rfind("<script id=edgeData type=application/json>")
        .expect("edgeData blob present");
    let rest = &html[blob_start..];
    let json = &rest[rest.find('>').unwrap() + 1..rest.find("</script>").unwrap()];
    let parsed: serde_json::Value =
        serde_json::from_str(json).expect("edgeData must stay valid JSON");
    assert_eq!(parsed["edges"].as_array().unwrap().len(), built.edges);
    assert_eq!(parsed["misses"].as_array().unwrap().len(), 0);

    // Minified but still a POSIX text file.
    assert!(html.ends_with('\n'));
}
