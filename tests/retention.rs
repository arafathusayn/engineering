//! Integration: exercise the actual compiled binary against an isolated
//! scratch root, proving sidecar retention end to end across real builds —
//! the orchestration in main.rs's build/check modes that unit tests, which
//! only reach its private helpers in isolation, can't cover. This is the
//! rollover-and-check scenario this session's manual test plan verified by
//! hand repeatedly; here it's a permanent regression guard instead.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn run_builder(canon_root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_canon-builder"))
        .args(args)
        .env("CANON_ROOT", canon_root)
        .output()
        .expect("failed to spawn canon-builder")
}

/// A scratch project root wired so the freshly-compiled binary's freshness
/// check passes: `src/`, `build.rs`, `Cargo.toml`, `Cargo.lock`, and the
/// HTML templates are symlinked to the real checkout (read-only, and
/// therefore fine to share), while `templates/app.js` is a real,
/// independently mutable copy this test can edit across iterations without
/// touching the actual project working tree.
fn seed_fixture(dir: &Path) {
    let repo = repo_root();
    fs::create_dir_all(dir.join("templates")).unwrap();
    for name in ["build.rs", "Cargo.toml", "Cargo.lock", "canon.toon"] {
        std::os::unix::fs::symlink(repo.join(name), dir.join(name)).unwrap();
    }
    std::os::unix::fs::symlink(repo.join("src"), dir.join("src")).unwrap();
    for entry in fs::read_dir(repo.join("templates")).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        let target = dir.join("templates").join(&name);
        if name == "app.js" {
            fs::copy(entry.path(), &target).unwrap();
        } else {
            std::os::unix::fs::symlink(entry.path(), &target).unwrap();
        }
    }
}

fn sidecar_names(dir: &Path) -> BTreeSet<String> {
    fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_str().unwrap().to_string())
        .filter(|n| n.starts_with("app.") && n.ends_with(".js"))
        .collect()
}

fn manifest_lines(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("sidecars.manifest"))
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn retention_rolls_over_across_real_builds_and_check_agrees_at_every_step() {
    let dir = std::env::temp_dir().join(format!(
        "canon-builder-retention-test-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    seed_fixture(&dir);

    // Four distinct content changes: SIDECAR_RETENTION = 3 means the last
    // one evicts the very first hash from both the manifest and disk.
    let mut hashes = Vec::new();
    for i in 0..4 {
        let app_js_path = dir.join("templates/app.js");
        let mut app_js = fs::read_to_string(&app_js_path).unwrap();
        app_js.push_str(&format!("\n// retention-test-{i}\n"));
        fs::write(&app_js_path, app_js).unwrap();

        let build = run_builder(&dir, &[]);
        assert!(
            build.status.success(),
            "build {i} failed: {}",
            String::from_utf8_lossy(&build.stderr)
        );
        hashes.push(manifest_lines(&dir).last().unwrap().clone());

        let check = run_builder(&dir, &["--check"]);
        assert!(
            check.status.success(),
            "check {i} failed: {}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
    assert_eq!(hashes.iter().collect::<BTreeSet<_>>().len(), 4, "each edit must change the hash");

    let expected: BTreeSet<String> = hashes[1..].iter().cloned().collect();
    assert_eq!(manifest_lines(&dir), hashes[1..].to_vec());
    assert_eq!(sidecar_names(&dir), expected);
    assert!(
        !dir.join(&hashes[0]).exists(),
        "the oldest hash must be evicted from disk, not just the manifest"
    );

    let _ = fs::remove_dir_all(&dir);
}
