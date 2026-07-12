//! Embeds the source hash (see src/source_hash.rs) into the binary as
//! CANON_SOURCE_HASH, so a shipped bin/canon-builder can prove at run time
//! that it was built from the sources in the checkout it operates on.

mod source_hash {
    include!("src/source_hash.rs");
}
use source_hash::source_hash;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=templates");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let hash = source_hash(root).expect("cannot hash source files");
    println!("cargo:rustc-env=CANON_SOURCE_HASH={hash}");
}
