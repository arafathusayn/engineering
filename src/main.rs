//! CLI for the canon builder.
//!
//! ```text
//! canon-builder                # write the minified index.html
//! canon-builder --check        # verify index.html matches the build
//! canon-builder --source-hash  # print the embedded source hash
//! ```
//!
//! Environment policy (root resolution, argument handling) lives here at
//! the edge; the library takes explicit paths.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result, bail};

enum Mode {
    Build,
    Check,
    SourceHash,
}

/// Total number of `app.<hash>.js` sidecars to keep on disk, including the
/// current build's own asset. A CDN or browser can still be serving a
/// cached index.html that references an older hash right after a new
/// deploy lands; pruning that sidecar immediately would 404 the page's
/// script until the HTML itself revalidates. Keeping a small trailing
/// window covers that overlap without accumulating sidecars forever.
const SIDECAR_RETENTION: usize = 3;

/// Filename of the manifest recording sidecar deploy order, oldest first,
/// one name per line. Order is tracked here explicitly and committed to git,
/// rather than inferred from filesystem mtimes: a fresh checkout stamps
/// every pre-existing file with the checkout time, so mtimes carry no
/// deploy-history information across the clone that every real build starts
/// from. The manifest is the single source of truth for "which sidecar is
/// oldest", readable and diffable like any other tracked file.
const SIDECAR_MANIFEST: &str = "sidecars.manifest";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args() -> Result<Mode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] => Ok(Mode::Build),
        ["--check"] => Ok(Mode::Check),
        ["--source-hash"] => Ok(Mode::SourceHash),
        other => {
            bail!("unknown arguments {other:?}\nusage: canon-builder [--check | --source-hash]")
        }
    }
}

/// Find the checkout to operate on: an explicit CANON_ROOT, else the first
/// location that holds BOTH runtime inputs (canon.toon and templates/app.js)
/// among the cwd, the checkout this binary sits in (bin/..), and the
/// compile-time manifest dir (covers `cargo run` from a subdirectory).
fn resolve_root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("CANON_ROOT") {
        return Ok(PathBuf::from(root));
    }
    let has_inputs = |dir: &std::path::Path| {
        dir.join("canon.toon").exists() && dir.join("templates/app.js").exists()
    };
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(repo) = exe.parent().and_then(|d| d.parent())
    {
        candidates.push(repo.to_path_buf());
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for dir in &candidates {
        if has_inputs(dir) {
            return Ok(dir.clone());
        }
    }
    bail!(
        "cannot find a project root (a directory with canon.toon and templates/app.js); \
         tried {:?}. Set CANON_ROOT to point at the checkout.",
        candidates
    )
}

fn run() -> Result<ExitCode> {
    let mode = parse_args()?;

    if let Mode::SourceHash = mode {
        println!("{}", canon_builder::embedded_source_hash());
        return Ok(ExitCode::SUCCESS);
    }

    let root = resolve_root()?;
    canon_builder::verify_freshness(&root)?;

    let built = canon_builder::build(&root)?;
    for warning in &built.warnings {
        eprintln!("warning: {warning}");
    }

    let out = canon_builder::output_path(&root);
    let current = built
        .assets
        .first()
        .map(|a| a.name.as_str())
        .context("build produced no sidecar assets")?;

    match mode {
        Mode::Check => {
            let mut ok = true;
            let committed = fs::read_to_string(&out)
                .with_context(|| format!("cannot read {}", out.display()))?;
            if committed != built.html {
                ok = false;
                let at = committed
                    .bytes()
                    .zip(built.html.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or_else(|| committed.len().min(built.html.len()));
                eprintln!(
                    "FAIL: index.html differs from build output at byte {at}; \
                     run the builder to regenerate"
                );
            }
            // The current sidecar must be present and byte-identical.
            for asset in &built.assets {
                match fs::read_to_string(root.join(&asset.name)) {
                    Ok(content) if content == asset.content => {}
                    Ok(_) => {
                        ok = false;
                        eprintln!("FAIL: {} differs from build output; regenerate", asset.name);
                    }
                    Err(_) => {
                        ok = false;
                        eprintln!("FAIL: {} is missing; run the builder", asset.name);
                    }
                }
            }
            let manifest = read_sidecar_manifest(&root)?;
            // The manifest must end with the current build's asset: that's
            // the only way `--check` can tell a committed sidecar set is
            // actually up to date rather than merely retention-sized.
            if manifest.last().map(String::as_str) != Some(current) {
                ok = false;
                eprintln!(
                    "FAIL: {SIDECAR_MANIFEST} does not end with {current}; run the builder"
                );
            }
            // A manifest that grew past the cap (e.g. by hand-editing) is
            // drift even if every entry it lists is individually valid.
            if manifest.len() > SIDECAR_RETENTION {
                ok = false;
                eprintln!(
                    "FAIL: {SIDECAR_MANIFEST} lists {} sidecars, over the \
                     {SIDECAR_RETENTION}-entry retention cap; run the builder to prune it",
                    manifest.len()
                );
            }
            // Every retained (non-current) sidecar the manifest lists must
            // still exist as a regular file; content is unverifiable (its
            // source is gone) but presence and shape are not. A directory
            // or symlink standing in for it does not count.
            for name in &manifest {
                if name != current && !is_regular_file(&root.join(name)) {
                    ok = false;
                    eprintln!(
                        "FAIL: {name} is listed in {SIDECAR_MANIFEST} but missing from disk"
                    );
                }
            }
            // Any sidecar-shaped file the manifest doesn't know about is
            // drift: running the builder deletes it (see the cleanup pass
            // in build mode below).
            let listed: BTreeSet<&str> = manifest.iter().map(String::as_str).collect();
            for path in existing_app_scripts(&root)? {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if !listed.contains(name) {
                    ok = false;
                    eprintln!(
                        "FAIL: sidecar {name} is not listed in {SIDECAR_MANIFEST}; \
                         run the builder to remove it"
                    );
                }
            }
            if ok {
                println!("ok: index.html and its sidecars match the canon.toon build output");
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::FAILURE)
            }
        }
        _ => {
            // Advance the manifest to end with the current asset, capping
            // the retention window, then remove every on-disk sidecar the
            // resulting manifest doesn't list: both entries that fell off
            // the window and any stray file the manifest never knew about.
            let manifest = read_sidecar_manifest(&root)?;
            let manifest = advance_manifest(manifest, current);
            let keep: BTreeSet<&str> = manifest.iter().map(String::as_str).collect();
            for path in existing_app_scripts(&root)? {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if !keep.contains(name) {
                    fs::remove_file(&path)
                        .with_context(|| format!("cannot remove {}", path.display()))?;
                }
            }
            fs::write(&out, &built.html)
                .with_context(|| format!("cannot write {}", out.display()))?;
            for asset in &built.assets {
                let path = root.join(&asset.name);
                fs::write(&path, &asset.content)
                    .with_context(|| format!("cannot write {}", path.display()))?;
            }
            write_sidecar_manifest(&root, &manifest)?;
            let names: Vec<&str> = built.assets.iter().map(|a| a.name.as_str()).collect();
            println!(
                "built index.html ({} bytes, minified) + [{}] from {} cards, {} categories, \
                 {} lineage entries, {} edges",
                built.html.len(),
                names.join(", "),
                built.cards,
                built.categories,
                built.lineage_entries,
                built.edges
            );
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Every `app.*.js` sidecar currently sitting in the checkout root.
fn existing_app_scripts(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))? {
        let entry = entry?;
        // Only regular files can be sidecars: skip directories and symlinks so a
        // lookalike never reaches remove_file (which would fail confusingly).
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if canon_builder::is_app_sidecar(name) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

/// Whether `path` is a regular file, without following a symlink at that
/// path — mirrors `existing_app_scripts`'s `DirEntry::file_type()` check, so
/// a directory or symlink standing in for a sidecar is never treated as a
/// deployable file.
fn is_regular_file(path: &std::path::Path) -> bool {
    fs::symlink_metadata(path)
        .map(|m| m.file_type().is_file())
        .unwrap_or(false)
}

/// Read `SIDECAR_MANIFEST` as an ordered list of sidecar names, oldest
/// first. A missing file (the very first build) is an empty history, not an
/// error. Every line must be a validly shaped sidecar name and appear only
/// once: the manifest is untrusted input the moment it can be hand-edited,
/// and its entries end up as `root.join(name)` arguments to `remove_file`,
/// so a malformed line (e.g. `../secret`) or a duplicate must never reach
/// that call.
fn read_sidecar_manifest(root: &std::path::Path) -> Result<Vec<String>> {
    let path = root.join(SIDECAR_MANIFEST);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for line in text.lines() {
        if !canon_builder::is_app_sidecar(line) {
            bail!("{SIDECAR_MANIFEST} contains an invalid entry: {line:?}");
        }
        if !seen.insert(line) {
            bail!("{SIDECAR_MANIFEST} lists {line} more than once");
        }
        names.push(line.to_string());
    }
    Ok(names)
}

fn write_sidecar_manifest(root: &std::path::Path, names: &[String]) -> Result<()> {
    let path = root.join(SIDECAR_MANIFEST);
    let mut text = names.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    fs::write(&path, text).with_context(|| format!("cannot write {}", path.display()))
}

/// Advance `names` (oldest first) to end with `current`: drop any earlier
/// occurrence of it first (a content revert reuses that old hash, and it
/// belongs at the end again, not duplicated), append it, then cap the front
/// so at most `SIDECAR_RETENTION` names remain. Pure and deterministic:
/// order comes from list position, never from a clock.
fn advance_manifest(mut names: Vec<String>, current: &str) -> Vec<String> {
    names.retain(|n| n != current);
    names.push(current.to_string());
    let overflow = names.len().saturating_sub(SIDECAR_RETENTION);
    names.drain(..overflow);
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "canon-builder-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn advance_appends_and_evicts_once_over_the_retention_cap() {
        let history = vec!["app.aaaaaaaaaaaa.js".to_string(), "app.bbbbbbbbbbbb.js".to_string(), "app.cccccccccccc.js".to_string()];
        let kept = advance_manifest(history, "app.dddddddddddd.js");
        // SIDECAR_RETENTION = 3: adding a 4th name evicts exactly the oldest.
        assert_eq!(kept, vec!["app.bbbbbbbbbbbb.js", "app.cccccccccccc.js", "app.dddddddddddd.js"]);
    }

    #[test]
    fn advance_is_a_no_op_below_the_retention_cap() {
        let kept = advance_manifest(vec!["app.aaaaaaaaaaaa.js".to_string()], "app.bbbbbbbbbbbb.js");
        assert_eq!(kept, vec!["app.aaaaaaaaaaaa.js", "app.bbbbbbbbbbbb.js"]);
    }

    #[test]
    fn advance_with_unchanged_content_leaves_history_untouched() {
        let history = vec!["app.aaaaaaaaaaaa.js".to_string(), "app.bbbbbbbbbbbb.js".to_string()];
        let kept = advance_manifest(history, "app.bbbbbbbbbbbb.js");
        assert_eq!(kept, vec!["app.aaaaaaaaaaaa.js", "app.bbbbbbbbbbbb.js"], "already-last entry moves nowhere");
    }

    #[test]
    fn advance_on_a_reverted_hash_moves_it_back_to_the_end_without_duplicating() {
        let history = vec!["app.aaaaaaaaaaaa.js".to_string(), "app.bbbbbbbbbbbb.js".to_string(), "app.cccccccccccc.js".to_string()];
        // Content reverted to what app.aaaaaaaaaaaa.js was built from earlier.
        let kept = advance_manifest(history, "app.aaaaaaaaaaaa.js");
        assert_eq!(
            kept,
            vec!["app.bbbbbbbbbbbb.js", "app.cccccccccccc.js", "app.aaaaaaaaaaaa.js"],
            "the reverted hash moves to the end, not duplicated"
        );
    }

    #[test]
    fn directories_and_symlinks_are_never_treated_as_sidecars() {
        let root = temp_root("non-files");
        fs::create_dir(root.join("app.aaaaaaaaaaaa.js")).unwrap();
        fs::write(root.join("app.bbbbbbbbbbbb.js"), b"x").unwrap();

        let found: Vec<String> = existing_app_scripts(&root)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        assert_eq!(found, vec!["app.bbbbbbbbbbbb.js"], "the directory is skipped");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn is_regular_file_rejects_directories_and_missing_paths() {
        let root = temp_root("regular-file");
        fs::write(root.join("real.js"), b"x").unwrap();
        fs::create_dir(root.join("dir.js")).unwrap();

        assert!(is_regular_file(&root.join("real.js")));
        assert!(!is_regular_file(&root.join("dir.js")));
        assert!(!is_regular_file(&root.join("missing.js")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manifest_rejects_a_line_that_is_not_sidecar_shaped() {
        let root = temp_root("bad-shape");
        // A malformed entry (here, a path-traversal attempt) must never
        // reach a caller that joins it onto `root` and deletes it.
        fs::write(root.join(SIDECAR_MANIFEST), "../secret\n").unwrap();

        let err = read_sidecar_manifest(&root).unwrap_err();
        assert!(err.to_string().contains("invalid entry"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manifest_rejects_a_duplicated_entry() {
        let root = temp_root("dup-entry");
        fs::write(
            root.join(SIDECAR_MANIFEST),
            "app.aaaaaaaaaaaa.js\napp.aaaaaaaaaaaa.js\n",
        )
        .unwrap();

        let err = read_sidecar_manifest(&root).unwrap_err();
        assert!(err.to_string().contains("more than once"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_manifest_reads_as_empty_history() {
        let root = temp_root("missing-manifest");
        assert_eq!(read_sidecar_manifest(&root).unwrap(), Vec::<String>::new());
        let _ = fs::remove_dir_all(&root);
    }
}
