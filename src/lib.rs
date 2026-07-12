//! Build the Engineering Canon page: canon.toon (content, the source of
//! truth) + templates/ (presentation) → a fully pre-rendered, minified
//! index.html that needs no JavaScript to read.

pub mod model;
pub mod text;
pub mod view;

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use askama::Template;

pub struct Build {
    pub html: String,
    pub warnings: Vec<String>,
    pub cards: usize,
    pub categories: usize,
    pub lineage_entries: usize,
    pub edges: usize,
}

/// The repository root: all inputs and the output live here. Resolved at
/// runtime so the shipped binary works anywhere, not just where it was
/// compiled: explicit `CANON_ROOT` first, then a cwd containing canon.toon,
/// then the checkout the binary sits in (`bin/..`), and finally the
/// compile-time manifest dir (covers `cargo run` from a subdirectory).
pub fn project_root() -> PathBuf {
    if let Some(root) = std::env::var_os("CANON_ROOT") {
        return PathBuf::from(root);
    }
    if let Ok(cwd) = std::env::current_dir()
        && cwd.join("canon.toon").exists()
    {
        return cwd;
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(repo) = exe.parent().and_then(|d| d.parent())
        && repo.join("canon.toon").exists()
    {
        return repo.to_path_buf();
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn output_path() -> PathBuf {
    project_root().join("index.html")
}

/// Decode, validate, derive, render, minify.
pub fn build() -> Result<Build> {
    let root = project_root();
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
        lineage_entries: derived.lineage_entries,
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
