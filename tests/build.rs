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
    // Locate the edgeData element without assuming how the minifier orders
    // or quotes attributes. Take the LAST candidate: the PARSE GUIDE comment
    // mentions the tag in prose before the real element appears.
    let (open_tag, after_tag) = html
        .match_indices("<script")
        .filter_map(|(start, _)| {
            let tag_end = start + html[start..].find('>')?;
            let tag = &html[start..=tag_end];
            (tag.contains("edgeData") && tag.contains("application/json"))
                .then_some((tag, &html[tag_end + 1..]))
        })
        .last()
        .expect("edgeData blob present");
    assert!(open_tag.contains("edgeData"), "sanity: {open_tag}");
    let json = &after_tag[..after_tag.find("</script>").expect("blob closes")];
    let parsed: serde_json::Value =
        serde_json::from_str(json).expect("edgeData must stay valid JSON");
    assert_eq!(parsed["edges"].as_array().unwrap().len(), built.edges);
    assert_eq!(parsed["misses"].as_array().unwrap().len(), 0);

    // Minified but still a POSIX text file.
    assert!(html.ends_with('\n'));
}
