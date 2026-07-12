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
    /// Content-hashed sidecar files emitted next to index.html (currently the
    /// externalized app script; see `externalize_app_script`).
    pub assets: Vec<Asset>,
    pub warnings: Vec<String>,
    pub cards: usize,
    pub categories: usize,
    pub lineage_entries: usize,
    pub edges: usize,
}

/// A file written alongside index.html. Its name embeds a content hash, so the
/// URL changes whenever the bytes do — the browser can cache it immutably and
/// still never serve a stale version after a rebuild.
pub struct Asset {
    pub name: String,
    pub content: String,
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
    // Lift the (now minified) app script out of the page into a content-hashed
    // sidecar so returning visitors can never run a stale cached copy: a change
    // to app.js flips the hash, hence the `<script src>` URL, so the browser
    // fetches the new file instead of reusing the old one. index.html itself
    // stays a single small document that revalidates on its ETag.
    let (html, assets) = externalize_app_script(html)?;

    Ok(Build {
        html,
        assets,
        warnings,
        cards: canon.cards.len(),
        categories: canon.cats.len(),
        lineage_entries: canon.lineage.len(),
        edges: derived.edges,
    })
}

/// Move the page's single bare `<script>…</script>` — the app script; the
/// edgeData blob is the only other script and always carries attributes — into
/// `app.<hash>.js`, replacing it with a `<script src>` reference. Runs after
/// minify, so the sidecar is the same minified JS that used to ship inline.
fn externalize_app_script(html: String) -> Result<(String, Vec<Asset>)> {
    const OPEN: &str = "<script>";
    const CLOSE: &str = "</script>";
    let open = html
        .rfind(OPEN)
        .context("no bare <script> to externalize in the built page")?;
    let start = open + OPEN.len();
    let close = html[start..]
        .find(CLOSE)
        .map(|i| start + i)
        .context("the app <script> is never closed")?;
    let js = html[start..close].to_string();
    let name = format!("app.{}.js", content_hash(js.as_bytes()));
    let tag = format!("<script src={name}></script>");
    let mut out = String::with_capacity(html.len());
    out.push_str(&html[..open]);
    out.push_str(&tag);
    out.push_str(&html[close + CLOSE.len()..]);
    Ok((out, vec![Asset { name, content: js }]))
}

/// FNV-1a 64 over the bytes, truncated to 12 hex digits — ample to flip the
/// filename on any content change (this guards cache freshness, not against a
/// motivated collision).
fn content_hash(bytes: &[u8]) -> String {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:016x}")[..12].to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn externalize_lifts_the_bare_script_into_a_hashed_sidecar() {
        // The edgeData blob (attributed) stays inline; only the bare app
        // script is externalized.
        let html = "<script id=edgeData type=application/json>{\"e\":1}</script>\
                    <script>console.log(1)</script>\n";
        let (out, assets) = externalize_app_script(html.to_string()).unwrap();
        assert_eq!(assets.len(), 1);
        let asset = &assets[0];
        assert_eq!(asset.content, "console.log(1)");
        assert!(asset.name.starts_with("app.") && asset.name.ends_with(".js"));
        assert!(out.contains(&format!("<script src={}></script>", asset.name)));
        assert!(!out.contains("<script>console.log"), "inline app script is gone");
        assert!(out.contains("id=edgeData"), "the JSON blob is untouched");
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn externalized_filename_tracks_content() {
        let one = "<script>a()</script>";
        let two = "<script>b()</script>";
        let name = |h: &str| externalize_app_script(h.to_string()).unwrap().1[0].name.clone();
        assert_eq!(name(one), name(one), "same JS → same URL (deterministic)");
        assert_ne!(name(one), name(two), "changed JS → busted URL");
    }

    #[test]
    fn content_hash_is_stable_content_sensitive_and_short() {
        assert_eq!(content_hash(b"abc"), content_hash(b"abc"));
        assert_ne!(content_hash(b"abc"), content_hash(b"abd"));
        assert_eq!(content_hash(b"abc").len(), 12);
    }
}
