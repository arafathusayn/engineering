//! CLI for the canon builder.
//!
//! ```text
//! cargo run --release            # write the minified index.html
//! cargo run --release -- --check # verify index.html matches the build
//! ```

use std::fs;
use std::process::ExitCode;

use anyhow::{Context, Result};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let check = std::env::args().any(|a| a == "--check");

    let built = canon_builder::build()?;
    for warning in &built.warnings {
        eprintln!("warning: {warning}");
    }

    let out = canon_builder::output_path();
    if check {
        let committed =
            fs::read_to_string(&out).with_context(|| format!("cannot read {}", out.display()))?;
        if committed == built.html {
            println!("ok: index.html matches the canon.toon build output");
            Ok(ExitCode::SUCCESS)
        } else {
            let at = committed
                .bytes()
                .zip(built.html.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or_else(|| committed.len().min(built.html.len()));
            eprintln!(
                "FAIL: index.html differs from build output at byte {at}; \
                 run `cargo run --release` to regenerate"
            );
            Ok(ExitCode::FAILURE)
        }
    } else {
        fs::write(&out, &built.html).with_context(|| format!("cannot write {}", out.display()))?;
        println!(
            "built index.html ({} bytes, minified) from {} cards, {} categories, \
             {} lineage entries, {} edges",
            built.html.len(),
            built.cards,
            built.categories,
            built.lineage_entries,
            built.edges
        );
        Ok(ExitCode::SUCCESS)
    }
}
