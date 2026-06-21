//! `xtask gen-assets`: (re)generate the committed shell completions and man
//! pages from the live clap definitions.
//!
//! The release archives bundle the `assets/` tree (see `dist-workspace.toml`)
//! so it must stay in lockstep with the CLIs. `gen-assets --check`
//! regenerates into `target/` and diffs against the committed files, failing
//! if they have drifted — wired into `just ci`.

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use clap::{Command, CommandFactory};
use clap_complete::{Shell, generate_to};
use clap_complete_nushell::Nushell;
use clap_mangen::Man;

use crate::workspace_root;

/// A shipped binary: its name plus a factory for its clap [`Command`].
type Binary = (&'static str, fn() -> Command);

/// Binaries whose completions + man pages ship in the release archives.
/// `xtask` itself is internal (`publish = false`), so it is deliberately
/// excluded.
const BINARIES: &[Binary] = &[
    // The umbrella binary first: its completions/man cover every subcommand
    // (fmt/lint/render/explain/lsp/…). The standalone fmt/lsp binaries still
    // ship, so their completions/man are kept too.
    ("aozora", aozora_cli::Cli::command),
    ("aozora-fmt", aozora_fmt::Cli::command),
    ("aozora-lsp", aozora_lsp::Cli::command),
];

/// Shells with first-class `clap_complete` generators (Nushell is handled
/// separately via `clap_complete_nushell`).
const SHELLS: &[Shell] = &[Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell];

/// Entry point: regenerate `assets/`, or (when `check`) verify it is current.
pub(crate) fn run(check: bool) -> Result<(), String> {
    let assets = workspace_root()?.join("assets");
    let comp = assets.join("completions");
    let man = assets.join("man");

    if check {
        return check_drift(&comp, &man);
    }
    reset_dir(&comp)?;
    reset_dir(&man)?;
    for &(name, factory) in BINARIES {
        write_completions(name, factory, &comp)?;
        write_man(name, factory, &man)?;
    }
    eprintln!("gen-assets: wrote completions + man pages under assets/");
    Ok(())
}

/// Write every shell's completion script for one binary into `dir`.
fn write_completions(name: &str, factory: fn() -> Command, dir: &Path) -> Result<(), String> {
    for &shell in SHELLS {
        let mut cmd = factory();
        generate_to(shell, &mut cmd, name, dir)
            .map_err(|e| format!("generate {shell} completions for {name}: {e}"))?;
    }
    let mut cmd = factory();
    generate_to(Nushell, &mut cmd, name, dir)
        .map_err(|e| format!("generate nushell completions for {name}: {e}"))?;
    Ok(())
}

/// Render one binary's man page to `dir/<name>.1`.
fn write_man(name: &str, factory: fn() -> Command, dir: &Path) -> Result<(), String> {
    let mut buf = Vec::new();
    Man::new(factory())
        .render(&mut buf)
        .map_err(|e| format!("render man page for {name}: {e}"))?;
    let path = dir.join(format!("{name}.1"));
    fs::write(&path, buf).map_err(|e| format!("write {}: {e}", path.display()))
}

/// `--check`: regenerate into `target/gen-assets-check/` and diff against the
/// committed `assets/`. Writes only under `target/`, never the tracked tree.
fn check_drift(comp: &Path, man: &Path) -> Result<(), String> {
    let scratch = workspace_root()?.join("target").join("gen-assets-check");
    let tmp_comp = scratch.join("completions");
    let tmp_man = scratch.join("man");
    reset_dir(&tmp_comp)?;
    reset_dir(&tmp_man)?;
    for &(name, factory) in BINARIES {
        write_completions(name, factory, &tmp_comp)?;
        write_man(name, factory, &tmp_man)?;
    }

    let mut stale = Vec::new();
    diff_dir(&tmp_comp, comp, &mut stale)?;
    diff_dir(&tmp_man, man, &mut stale)?;
    if stale.is_empty() {
        eprintln!("gen-assets: assets/ is up to date");
        return Ok(());
    }
    Err(format!(
        "assets/ is stale ({} file(s) differ: {}).\n  \
         run `just gen-assets` and commit the result",
        stale.len(),
        stale.join(", ")
    ))
}

/// Collect into `stale` any file that differs between the freshly-generated
/// `generated` dir and the committed `tracked` dir (in either direction).
fn diff_dir(generated: &Path, tracked: &Path, stale: &mut Vec<String>) -> Result<(), String> {
    for name in entries(generated)? {
        let fresh = fs::read(generated.join(&name)).map_err(|e| format!("read generated: {e}"))?;
        let committed = fs::read(tracked.join(&name)).unwrap_or_default();
        if committed != fresh {
            stale.push(name.to_string_lossy().into_owned());
        }
    }
    for name in entries(tracked)? {
        if !generated.join(&name).exists() {
            stale.push(name.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

/// Sorted file names directly under `dir` (empty if `dir` is absent).
fn entries(dir: &Path) -> Result<Vec<OsString>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        names.push(entry.file_name());
    }
    names.sort();
    Ok(names)
}

/// Remove `dir` and recreate it empty so stale outputs never linger.
fn reset_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| format!("clear {}: {e}", dir.display()))?;
    }
    fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))
}
