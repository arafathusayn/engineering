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
use std::io::Write;
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
/// window covers the common case (one deploy landing while a visitor's
/// page is still cached) without accumulating sidecars forever. It is a
/// fixed count, not a cache-lifetime guarantee: enough rapid successive
/// content changes can still evict a sidecar a lingering cached page needs.
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
            // Every listed sidecar must be a regular file whose bytes still
            // hash to its own filename. For `current` this is redundant
            // with the byte-for-byte check against the fresh build above —
            // its filename's hash is derived from those exact bytes by
            // construction — but skipping it isn't a shortcut, it's a hole:
            // `read_to_string` above follows a symlink, so without this
            // check too, a symlinked current entry whose target happens to
            // hold matching bytes would pass despite not being a regular
            // file at all.
            for name in &manifest {
                if let Err(e) = verify_retained_sidecar(&root.join(name), name) {
                    ok = false;
                    eprintln!("FAIL: {e:#}");
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
            // Refuse to proceed if a retained entry is unusable: a
            // directory or symlink in its place, content that no longer
            // hashes to its own filename, or (for a predecessor only —
            // `current` is written fresh below, so absence there is fine) a
            // file that has simply gone missing. Without this, the cleanup
            // pass below has no way to catch it: it only ever deletes
            // *unlisted* entries, so anything still named in the manifest —
            // valid or not — is preserved by design, written back out as if
            // fine, and would only surface as a failure on the next
            // `--check` — fail here, before any output is written, instead.
            for name in &manifest {
                if name == current {
                    ensure_sidecar_path_is_writable(&root.join(name))?;
                } else {
                    verify_retained_sidecar(&root.join(name), name).with_context(|| {
                        format!(
                            "retained sidecar {name} in {SIDECAR_MANIFEST} is unusable; fix \
                             or remove it by hand before rebuilding"
                        )
                    })?;
                }
            }
            // Publish before pruning, and in dependency order: the current
            // sidecar first, then index.html (which references it), then
            // the manifest. If a write fails or the process is killed
            // partway, whatever's already published is self-consistent —
            // index.html is never made visible ahead of the asset it
            // names, and nothing has been deleted yet either way.
            for asset in &built.assets {
                let path = root.join(&asset.name);
                write_atomic(&path, asset.content.as_bytes())?;
            }
            write_atomic(&out, built.html.as_bytes())?;
            write_sidecar_manifest(&root, &manifest)?;
            let keep: BTreeSet<&str> = manifest.iter().map(String::as_str).collect();
            for path in existing_app_scripts(&root)? {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if !keep.contains(name) {
                    fs::remove_file(&path)
                        .with_context(|| format!("cannot remove {}", path.display()))?;
                }
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

/// Every `app.*.js`-shaped entry currently sitting in the checkout root:
/// regular files and symlinks alike, but never a directory (`remove_file`
/// can't remove one and would fail confusingly). A symlink is included
/// deliberately: `remove_file` unlinks the directory entry itself and never
/// follows it, so removing a stray one here is safe, and excluding it would
/// leave an unlisted symlink invisible to both cleanup and drift detection
/// forever. Whether a *listed* entry is actually usable — a real regular
/// file, not a symlink — is a separate question, checked by
/// `is_regular_file` / `verify_retained_sidecar`.
fn existing_app_scripts(root: &std::path::Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("cannot read {}", root.display()))? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
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

/// Refuse a sidecar path that already exists as something other than a
/// regular file. Only meaningful for the current build's own asset: an
/// absent path is fine there (build mode is about to write it), whereas a
/// directory or symlink standing in its place is not. A retained
/// predecessor gets no such exemption — build mode never recreates one, so
/// its absence is a hard error, checked separately with
/// `verify_retained_sidecar`.
fn ensure_sidecar_path_is_writable(path: &std::path::Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => Ok(()),
        Ok(_) => bail!("{} exists but is not a regular file", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("cannot stat {}", path.display())),
    }
}

/// The hash portion of a sidecar filename (`app.<hash>.js`). The caller
/// must already know `name` is sidecar-shaped (every manifest entry is,
/// by construction — `read_sidecar_manifest` validates each one through
/// `is_app_sidecar` before it's ever stored).
fn sidecar_hash(name: &str) -> &str {
    name.strip_prefix("app.")
        .and_then(|s| s.strip_suffix(".js"))
        .expect("caller guarantees name is sidecar-shaped")
}

/// Verify a retained predecessor: a regular file whose bytes still hash to
/// the value embedded in its own name. This is the only integrity check
/// available for a predecessor — its original source is gone, so there's
/// nothing to compare its content against except itself. A mismatch means
/// the file was corrupted or hand-edited after being written; either way, a
/// browser holding a cached reference to this hash would silently receive
/// the wrong script.
fn verify_retained_sidecar(path: &std::path::Path, name: &str) -> Result<()> {
    if !is_regular_file(path) {
        bail!("{name} is missing or not a regular file");
    }
    let bytes = fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if canon_builder::content_hash(&bytes) != sidecar_hash(name) {
        bail!("{name}'s content no longer matches its own hash");
    }
    Ok(())
}

/// Read `SIDECAR_MANIFEST` as an ordered list of sidecar names, oldest
/// first. A missing file (the very first build) is an empty history, not an
/// error — but a symlink at that path is rejected outright, dangling or
/// not: `read_to_string` follows it, so a dangling symlink would otherwise
/// silently read as "no manifest" instead of the corrupted state it is.
/// Every line must be a validly shaped sidecar name and appear only once:
/// the manifest is untrusted input the moment it can be hand-edited, and
/// its entries are joined to `root` for filesystem operations (existence,
/// content, and regular-file checks) and compared for set membership during
/// cleanup — an exact basename shape rules out path traversal
/// (`../secret`), and a duplicate would make "which sidecar is retained"
/// ambiguous.
fn read_sidecar_manifest(root: &std::path::Path) -> Result<Vec<String>> {
    let path = root.join(SIDECAR_MANIFEST);
    let text = match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_symlink() => {
            bail!("{SIDECAR_MANIFEST} is a symlink; remove it by hand before rebuilding")
        }
        Ok(_) => fs::read_to_string(&path)
            .with_context(|| format!("cannot read {}", path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e).with_context(|| format!("cannot stat {}", path.display())),
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

/// Write `contents` to `path` via a same-directory temp file plus an atomic
/// rename, rather than an in-place `fs::write`. Three reasons: a rename
/// replaces whatever sits at `path` — file or symlink — as a single
/// directory-entry swap, so the *final* path can never be written through
/// to an unrelated target; a process killed mid-write leaves either the old
/// file or the fully-written new one, never a half-written one; and the
/// temp path itself is created with `create_new`, which fails if anything —
/// including a symlink planted at that predictable name — already exists
/// there, rather than following it. Used for every published artifact (the
/// current sidecar, index.html, the manifest), so a reader never observes a
/// partial version of any of them.
fn write_atomic(path: &std::path::Path, contents: &[u8]) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("{} has no file name", path.display()))?;
    let tmp = path.with_file_name(format!("{name}.tmp.{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .with_context(|| format!("cannot create {}", tmp.display()))?;
    file.write_all(contents)
        .with_context(|| format!("cannot write {}", tmp.display()))?;
    drop(file);
    fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))
}

fn write_sidecar_manifest(root: &std::path::Path, names: &[String]) -> Result<()> {
    let mut text = names.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    write_atomic(&root.join(SIDECAR_MANIFEST), text.as_bytes())
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
    fn existing_app_scripts_skips_directories_but_includes_symlinks() {
        let root = temp_root("non-files");
        fs::create_dir(root.join("app.aaaaaaaaaaaa.js")).unwrap();
        fs::write(root.join("app.bbbbbbbbbbbb.js"), b"x").unwrap();
        #[cfg(unix)]
        {
            fs::write(root.join("real-target.js"), b"x").unwrap();
            std::os::unix::fs::symlink(root.join("real-target.js"), root.join("app.cccccccccccc.js")).unwrap();
        }

        let found: Vec<String> = existing_app_scripts(&root)
            .unwrap()
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_string())
            .collect();
        // The directory is skipped (remove_file can't remove one); the
        // symlink is a candidate like any regular file, so an unlisted one
        // is actually reachable by cleanup and drift detection.
        #[cfg(unix)]
        assert_eq!(found, vec!["app.bbbbbbbbbbbb.js", "app.cccccccccccc.js"]);
        #[cfg(not(unix))]
        assert_eq!(found, vec!["app.bbbbbbbbbbbb.js"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn is_regular_file_rejects_directories_symlinks_and_missing_paths() {
        let root = temp_root("regular-file");
        fs::write(root.join("real.js"), b"x").unwrap();
        fs::create_dir(root.join("dir.js")).unwrap();

        assert!(is_regular_file(&root.join("real.js")));
        assert!(!is_regular_file(&root.join("dir.js")));
        assert!(!is_regular_file(&root.join("missing.js")));

        #[cfg(unix)]
        {
            // A symlink whose target happens to hold matching bytes must
            // still be rejected: it is not a deployable regular file.
            std::os::unix::fs::symlink(root.join("real.js"), root.join("link.js")).unwrap();
            assert!(!is_regular_file(&root.join("link.js")));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ensure_sidecar_path_is_writable_allows_absent_and_regular_files_only() {
        let root = temp_root("writable-guard");
        fs::write(root.join("real.js"), b"x").unwrap();
        fs::create_dir(root.join("dir.js")).unwrap();

        assert!(ensure_sidecar_path_is_writable(&root.join("missing.js")).is_ok());
        assert!(ensure_sidecar_path_is_writable(&root.join("real.js")).is_ok());
        assert!(ensure_sidecar_path_is_writable(&root.join("dir.js")).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("real.js"), root.join("link.js")).unwrap();
            assert!(ensure_sidecar_path_is_writable(&root.join("link.js")).is_err());
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_sidecar_manifest_round_trips_and_leaves_no_temp_file_behind() {
        let root = temp_root("manifest-write");
        let names = vec!["app.aaaaaaaaaaaa.js".to_string(), "app.bbbbbbbbbbbb.js".to_string()];

        write_sidecar_manifest(&root, &names).unwrap();
        assert_eq!(read_sidecar_manifest(&root).unwrap(), names);
        let leftovers: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_str().unwrap().to_string())
            .filter(|n| n != SIDECAR_MANIFEST)
            .collect();
        assert!(leftovers.is_empty(), "no .tmp file should survive a successful write: {leftovers:?}");

        // A second write (simulating a later build) replaces it cleanly.
        write_sidecar_manifest(&root, &["app.cccccccccccc.js".to_string()]).unwrap();
        assert_eq!(read_sidecar_manifest(&root).unwrap(), vec!["app.cccccccccccc.js"]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn write_atomic_refuses_to_write_through_a_pre_planted_temp_symlink() {
        let root = temp_root("write-atomic-race");
        let out = root.join("out.txt");

        // Simulate an attacker (or leftover cruft) pre-placing a symlink at
        // the exact temp path write_atomic is about to use, pointing
        // somewhere it has no business touching.
        let victim_dir = temp_root("write-atomic-victim");
        let victim = victim_dir.join("victim.txt");
        let tmp_path = root.join(format!("out.txt.tmp.{}", std::process::id()));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&victim, &tmp_path).unwrap();

            let err = write_atomic(&out, b"payload").unwrap_err();
            assert!(err.to_string().contains("cannot create"));
            assert!(!victim.exists(), "the symlink's target must never be written");
            assert!(!out.exists(), "the real destination must not be created either");
        }

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&victim_dir);
    }

    #[test]
    fn manifest_read_rejects_a_symlink_dangling_or_not() {
        let root = temp_root("manifest-symlink");
        #[cfg(unix)]
        {
            // Dangling: read_to_string would see NotFound and silently read
            // this as "no manifest" without the explicit symlink check.
            std::os::unix::fs::symlink(root.join("nowhere"), root.join(SIDECAR_MANIFEST)).unwrap();
            let err = read_sidecar_manifest(&root).unwrap_err();
            assert!(err.to_string().contains("symlink"));

            // Pointing at real content doesn't change the verdict either.
            fs::write(root.join("real-manifest"), "app.aaaaaaaaaaaa.js\n").unwrap();
            fs::remove_file(root.join(SIDECAR_MANIFEST)).unwrap();
            std::os::unix::fs::symlink(root.join("real-manifest"), root.join(SIDECAR_MANIFEST)).unwrap();
            let err = read_sidecar_manifest(&root).unwrap_err();
            assert!(err.to_string().contains("symlink"));
        }

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_retained_sidecar_catches_corruption_and_absence() {
        let root = temp_root("retained-integrity");
        let content = b"console.log(1)";
        let name = format!("app.{}.js", canon_builder::content_hash(content));
        fs::write(root.join(&name), content).unwrap();
        assert!(verify_retained_sidecar(&root.join(&name), &name).is_ok());

        // Tamper with the bytes without renaming the file: the name's hash
        // promise no longer holds.
        fs::write(root.join(&name), b"console.log(2)").unwrap();
        let err = verify_retained_sidecar(&root.join(&name), &name).unwrap_err();
        assert!(err.to_string().contains("no longer matches"));

        let missing = "app.000000000000.js";
        let err = verify_retained_sidecar(&root.join(missing), missing).unwrap_err();
        assert!(err.to_string().contains("missing or not a regular file"));

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
