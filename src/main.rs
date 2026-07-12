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
            let expected: BTreeSet<&str> =
                built.assets.iter().map(|a| a.name.as_str()).collect();
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
            // A sidecar from an earlier hash must never linger in the deploy.
            for path in existing_app_scripts(&root)? {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
                if !expected.contains(name) {
                    ok = false;
                    eprintln!("FAIL: stale sidecar {name} present; run the builder to remove it");
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
            // Refresh the content-hashed sidecars: drop any previous app.*.js
            // (a stale hash would otherwise ship forever) before writing the
            // current set, so the deploy holds exactly one app.<hash>.js.
            for stale in existing_app_scripts(&root)? {
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
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if canon_builder::is_app_sidecar(name) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}
