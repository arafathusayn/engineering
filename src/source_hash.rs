// Hash of the source files that are compiled INTO the builder binary
// (Rust sources, the Askama templates, the manifests). build.rs embeds it
// at compile time; at run time the binary recomputes it from the checkout
// and refuses to operate when they differ — a stale bin/canon-builder can
// never silently build or validate the page.
//
// canon.toon and templates/app.js are deliberately excluded: they are read
// at run time, so the binary is never stale with respect to them.
//
// This file is `include!`d by build.rs and used as a module by the crate
// (plain `//` comments for that reason), so it must stay dependency-free
// (std only). FNV-1a is plenty here: the hash protects against accidental
// staleness, not tampering (tampering is addressed by CI's source lane).

use std::io;
use std::path::Path;

/// Files under `root` whose content shapes the binary's behavior.
fn hash_inputs(root: &Path) -> io::Result<Vec<std::path::PathBuf>> {
    let mut files = vec![
        root.join("build.rs"),
        root.join("Cargo.toml"),
        root.join("Cargo.lock"),
    ];
    for dir in ["src", "templates"] {
        for entry in std::fs::read_dir(root.join(dir))? {
            let path = entry?.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let compiled_in = (dir == "src" && name.ends_with(".rs"))
                || (dir == "templates" && name.ends_with(".html"));
            if compiled_in {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// FNV-1a 64 over (relative path, content) of every compiled-in source file.
pub fn source_hash(root: &Path) -> io::Result<String> {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            hash ^= u64::from(b);
            hash = hash.wrapping_mul(PRIME);
        }
    };
    for path in hash_inputs(root)? {
        let rel = path.strip_prefix(root).unwrap_or(&path);
        eat(rel.to_string_lossy().as_bytes());
        eat(&[0]);
        eat(&std::fs::read(&path)?);
        eat(&[0xFF]);
    }
    Ok(format!("{hash:016x}"))
}
