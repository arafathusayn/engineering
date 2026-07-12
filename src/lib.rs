//! Build the Engineering Canon page: canon.toon (content, the source of
//! truth) + templates/ (presentation) → a fully pre-rendered, minified
//! index.html that needs no JavaScript to read.
//!
//! The library is pure with respect to its environment: callers resolve the
//! project root (see `main.rs`) and pass it in.

pub mod model;
mod source_hash;
pub mod text;
pub mod view;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use askama::Template;

pub struct Build {
    pub html: String,
    pub warnings: Vec<String>,
    pub cards: usize,
    pub categories: usize,
    pub lineage_entries: usize,
    pub edges: usize,
}

pub fn output_path(root: &Path) -> PathBuf {
    root.join("index.html")
}

/// The source hash this binary was compiled from (see src/source_hash.rs).
pub fn embedded_source_hash() -> &'static str {
    env!("CANON_SOURCE_HASH")
}

/// Refuse to run from a binary that no longer matches the checkout's
/// sources: a stale builder would build and validate the page with outdated
/// logic and templates, and its own `--check` would happily agree with it.
pub fn verify_freshness(root: &Path) -> Result<()> {
    let actual = source_hash::source_hash(root)
        .with_context(|| format!("cannot hash sources under {}", root.display()))?;
    if actual != embedded_source_hash() {
        bail!(
            "this canon-builder binary is stale: it was built from sources hashing \
             {}, but the checkout hashes {actual}. Rebuild and re-ship it:\n  \
             cargo build --profile dist --target x86_64-unknown-linux-musl\n  \
             cp target/x86_64-unknown-linux-musl/dist/canon-builder bin/canon-builder",
            embedded_source_hash()
        );
    }
    Ok(())
}

/// Decode, validate, derive, render, minify.
pub fn build(root: &Path) -> Result<Build> {
    let canon = model::load(&root.join("canon.toon"))?;
    let mut warnings = model::validate(&canon)?;

    let script_path = root.join("templates/app.js");
    let script = fs::read_to_string(&script_path)
        .with_context(|| format!("cannot read {}", script_path.display()))?;

    let derived = view::derive(&canon, script)?;
    warnings.extend(derived.warnings);

    let html = derived.page.render().context("template rendering failed")?;
    let html = minify(&html);

    Ok(Build {
        html,
        warnings,
        cards: canon.cards.len(),
        categories: canon.cats.len(),
        lineage_entries: canon.lineage.len(),
        edges: derived.edges,
    })
}

/// Minify the rendered page. Comments are kept — the leading PARSE GUIDE
/// documents the file for both humans and parsers; CSS and JS are minified
/// by lightningcss/oxc inside minify-html.
fn minify(html: &str) -> String {
    let cfg = minify_html::Cfg {
        keep_comments: true,
        minify_css: true,
        minify_js: true,
        ..minify_html::Cfg::default()
    };
    let mut out = minify_html::minify(html.as_bytes(), &cfg);
    if out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    String::from_utf8(out).expect("minifier must preserve UTF-8")
}
