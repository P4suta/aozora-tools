//! Build script: embed the pinned `aozora` parser rev + tag so the binary's
//! `--version` can report which upstream parser is compiled in.
//!
//! The workspace `Cargo.lock` is the resolved source of truth for the git
//! rev (the `Cargo.toml` pin and the lock always agree after `cargo build`).
//! Parsed with the `toml` crate into typed structs — robust to lockfile
//! formatting and never re-invokes cargo.

use std::path::Path;
use std::{env, fs};

use serde::Deserialize;

#[derive(Deserialize)]
struct Lockfile {
    #[serde(default)]
    package: Vec<LockPackage>,
}

#[derive(Deserialize)]
struct LockPackage {
    name: String,
    version: String,
    source: Option<String>,
}

fn main() {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set by cargo");
    // `crates/<crate>/` → the workspace root is two levels up.
    let root = Path::new(&manifest)
        .parent()
        .and_then(Path::parent)
        .expect("crate dir has a workspace-root grandparent");
    let lock = root.join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());

    let (rev, tag) =
        aozora_pin(&lock).unwrap_or_else(|| ("unknown".to_owned(), "unknown".to_owned()));
    println!("cargo:rustc-env=AOZORA_REV={rev}");
    println!("cargo:rustc-env=AOZORA_TAG={tag}");
}

/// Resolve the `(short-rev, "vX.Y.Z")` of the locked `aozora` package.
fn aozora_pin(lock: &Path) -> Option<(String, String)> {
    let text = fs::read_to_string(lock).ok()?;
    let parsed: Lockfile = toml::from_str(&text).ok()?;
    let pkg = parsed.package.into_iter().find(|p| p.name == "aozora")?;
    // Git sources serialise as `git+<url>?rev=<rev>#<locked-commit>`; the
    // fragment after `#` is the exact resolved commit.
    let commit = pkg.source.as_deref()?.rsplit('#').next()?;
    let short = commit.get(..7).unwrap_or(commit).to_owned();
    Some((short, format!("v{}", pkg.version)))
}
