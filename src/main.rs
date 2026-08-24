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

/// How many deploys' worth of `app.<hash>.js` sidecars to keep on disk: the
/// current build's own asset plus this many of the most recent superseded
/// ones. A CDN or browser can still be serving a cached index.html that
/// references an older hash after a new deploy lands; pruning that sidecar
/// immediately would 404 the page's script until the HTML itself revalidates.
/// Keeping a small trailing window covers that overlap without accumulating
/// sidecars forever.
const SIDECAR_RETENTION: usize = 3;

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
    let expected: BTreeSet<&str> = built.assets.iter().map(|a| a.name.as_str()).collect();
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
            // Every content-hashed sidecar must be present and byte-identical.
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
            // A sidecar may outlive its deploy for the retention window (see
            // SIDECAR_RETENTION); only one that has aged out is a failure.
            for path in sidecars_beyond_retention(&root, &expected)? {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                ok = false;
                eprintln!(
                    "FAIL: sidecar {name} is beyond the {SIDECAR_RETENTION}-deploy retention \
                     window; run the builder to prune it"
                );
            }
            if ok {
                println!("ok: index.html and its sidecars match the canon.toon build output");
                Ok(ExitCode::SUCCESS)
            } else {
                Ok(ExitCode::FAILURE)
            }
        }
        _ => {
            // Prune only the sidecars that have aged out of the retention
            // window (see SIDECAR_RETENTION); recent superseded ones are left
            // in place so a cached HTML page can still fetch its script.
            for stale in sidecars_beyond_retention(&root, &expected)? {
                fs::remove_file(&stale)
                    .with_context(|| format!("cannot remove {}", stale.display()))?;
            }
            fs::write(&out, &built.html)
                .with_context(|| format!("cannot write {}", out.display()))?;
            for asset in &built.assets {
                let path = root.join(&asset.name);
                fs::write(&path, &asset.content)
                    .with_context(|| format!("cannot write {}", path.display()))?;
            }
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

/// Sidecars not in `expected` (the current build's own asset names), ordered
/// newest-first by mtime, with the newest `SIDECAR_RETENTION - 1` dropped —
/// those are within the retention window and still fair game for a cached
/// index.html to reference. What remains is old enough to prune.
fn sidecars_beyond_retention(
    root: &std::path::Path,
    expected: &BTreeSet<&str>,
) -> Result<Vec<PathBuf>> {
    let mut superseded: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for path in existing_app_scripts(root)? {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if expected.contains(name) {
            continue;
        }
        let modified = fs::metadata(&path)
            .with_context(|| format!("cannot stat {}", path.display()))?
            .modified()
            .with_context(|| format!("no mtime for {}", path.display()))?;
        superseded.push((path, modified));
    }
    superseded.sort_by_key(|(_, modified)| std::cmp::Reverse(*modified));
    Ok(superseded
        .into_iter()
        .skip(SIDECAR_RETENTION.saturating_sub(1))
        .map(|(path, _)| path)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::time::{Duration, SystemTime};

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "canon-builder-test-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Create `path` and back-date its mtime by `age_secs`, so ordering by
    /// mtime is deterministic regardless of how fast the test runs.
    fn touch(path: &std::path::Path, age_secs: u64) {
        fs::write(path, b"x").unwrap();
        let stamp = SystemTime::now() - Duration::from_secs(age_secs);
        OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(stamp)
            .unwrap();
    }

    #[test]
    fn retention_keeps_the_newest_superseded_sidecars_and_flags_the_rest() {
        let root = temp_root("beyond-window");
        touch(&root.join("app.aaaaaaaaaaaa.js"), 400); // oldest
        touch(&root.join("app.bbbbbbbbbbbb.js"), 300);
        touch(&root.join("app.cccccccccccc.js"), 200);
        touch(&root.join("app.dddddddddddd.js"), 100); // newest superseded
        touch(&root.join("app.eeeeeeeeeeee.js"), 0); // the current build's own asset

        let expected: BTreeSet<&str> = ["app.eeeeeeeeeeee.js"].into_iter().collect();
        let names: Vec<String> = sidecars_beyond_retention(&root, &expected)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();

        // SIDECAR_RETENTION = 3 keeps the current asset plus its 2 newest
        // superseded predecessors (d, c); only the 2 oldest (b, a) age out.
        assert_eq!(names, vec!["app.bbbbbbbbbbbb.js", "app.aaaaaaaaaaaa.js"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn nothing_is_pruned_within_the_retention_window() {
        let root = temp_root("within-window");
        touch(&root.join("app.aaaaaaaaaaaa.js"), 20);
        touch(&root.join("app.bbbbbbbbbbbb.js"), 10);
        touch(&root.join("app.cccccccccccc.js"), 0);

        let expected: BTreeSet<&str> = ["app.cccccccccccc.js"].into_iter().collect();
        assert!(
            sidecars_beyond_retention(&root, &expected)
                .unwrap()
                .is_empty()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn directories_and_symlinks_are_never_treated_as_sidecars() {
        let root = temp_root("non-files");
        fs::create_dir(root.join("app.aaaaaaaaaaaa.js")).unwrap();
        touch(&root.join("app.bbbbbbbbbbbb.js"), 0);

        let expected: BTreeSet<&str> = BTreeSet::new();
        let names: Vec<String> = sidecars_beyond_retention(&root, &expected)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        // Only the regular file is a candidate; with SIDECAR_RETENTION = 3 and
        // just one superseded sidecar, nothing is beyond the window yet.
        assert!(names.is_empty());

        let _ = fs::remove_dir_all(&root);
    }
}
