//! `xtask coverage` — wraps `cargo llvm-cov` for the workspace.
//!
//! Stable-toolchain story:
//!
//! - `--branch` (LLVM branch coverage) requires the nightly toolchain
//!   and is not available on stable; the wrapper does NOT pass it.
//! - **Region coverage** is the finest granularity stable Rust ships:
//!   one counter per LLVM MC region, which covers each `if` / `match`
//!   arm independently. It is the closest stable analogue of C1
//!   (branch) coverage and is what the gate floor uses.
//! - Functions, lines, and regions all have `--fail-under-…`
//!   thresholds; this wrapper exposes the two we gate on (`lines`,
//!   `regions`).
//!
//! Workflow:
//!   1. `cargo llvm-cov clean --workspace` — drop stale `.profraw`s.
//!   2. `cargo llvm-cov [nextest] --workspace --all-features --no-report`
//!      — run the test suite under instrumentation. Uses nextest when
//!      `--nextest` is passed (CI does), otherwise the default test
//!      runner (so contributors don't need cargo-nextest installed).
//!   3. One or more `cargo llvm-cov report --…` invocations to produce
//!      whichever reports the caller asked for (HTML by default, plus
//!      optional `lcov.info` and stdout summary).
//!
//! Centralising the flag set here keeps local + CI numbers
//! comparable.

use std::env;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use clap::Args;

use crate::expect_status;

/// Filename regex ignored across both the run + report stages.
///
/// - `aozora-tools-xtask/src/.*` — developer tooling, not shipped to
///   end users; testing it like production code would be ceremonial.
/// - `aozora-(lsp|fmt)/src/main\.rs` — process entry points that do
///   `clap::Parser::parse()` and dispatch to library code which is
///   covered through other tests. Coverage of `fn main` itself is
///   not load-bearing.
///
/// Centralising the pattern here means CI and local runs can't
/// drift on which files are in / out of the denominator.
const IGNORE_FILENAME_REGEX: &str = r"aozora-tools-xtask/src/.*|aozora-(lsp|fmt)/src/main\.rs";

#[derive(Args, Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "CLI flag struct: each bool maps to one --flag; refactoring into a state-machine enum would lose clap's `--html` / `--no-html` toggle ergonomics"
)]
pub(crate) struct CoverageArgs {
    /// Emit an HTML report under `target/llvm-cov/html/` (default).
    /// Pass `--no-html` to skip when only lcov / summary is wanted.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    html: bool,

    /// Also emit `target/llvm-cov/lcov.info` (CI artifact format).
    #[arg(long, default_value_t = false)]
    lcov: bool,

    /// Print the per-file summary table on stdout.
    #[arg(long, default_value_t = false)]
    summary: bool,

    /// Open the HTML report in `$BROWSER` after generating it.
    #[arg(long, default_value_t = false)]
    open: bool,

    /// Use cargo-nextest as the test runner instead of the default
    /// `cargo test`. CI passes this; locally it requires
    /// `cargo install cargo-nextest --locked`.
    #[arg(long, default_value_t = false)]
    nextest: bool,

    /// Fail the command if line coverage falls below this percentage.
    /// Mirrors the CI gate so local runs reproduce the failure mode.
    #[arg(long)]
    fail_under_lines: Option<u32>,

    /// Fail the command if **region** coverage falls below this
    /// percentage. Region coverage is the closest stable analogue of
    /// C1 (branch) coverage; LLVM creates one region per MC piece so
    /// each `if` / `match` arm is counted independently.
    #[arg(long)]
    fail_under_regions: Option<u32>,
}

pub(crate) fn run(args: &CoverageArgs) -> Result<(), String> {
    ensure_llvm_cov_installed()?;
    clean_profraws()?;
    // `cargo llvm-cov report --lcov --output-path …/lcov.info` does
    // not auto-create its parent directory; ensure it exists after
    // `clean` (which can wipe `target/llvm-cov/`) so the report step
    // fails loudly with "no coverage data" rather than silently with
    // "No such file or directory".
    fs::create_dir_all(Path::new("target/llvm-cov/html"))
        .map_err(|err| format!("failed to mkdir -p target/llvm-cov/html: {err}"))?;
    run_instrumented_tests(args.nextest)?;

    if args.html {
        emit_html(args.open)?;
    }
    if args.lcov {
        emit_lcov()?;
    }
    if args.summary {
        emit_summary()?;
    }

    // Gate(s) run last so the report still exists on disk for the
    // failure-investigation cycle.
    if let Some(min) = args.fail_under_lines {
        gate("--fail-under-lines", min)?;
    }
    if let Some(min) = args.fail_under_regions {
        gate("--fail-under-regions", min)?;
    }

    Ok(())
}

fn clean_profraws() -> Result<(), String> {
    expect_status(
        cargo()
            .args(["llvm-cov", "clean", "--workspace"])
            .status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov clean: {e}"))?,
        "cargo llvm-cov clean",
    )
}

// Single instrumented test pass. `--no-report` defers report
// generation so multiple report formats (html + lcov + summary) can
// be produced from one set of `.profraw`s without re-running.
fn run_instrumented_tests(use_nextest: bool) -> Result<(), String> {
    let mut cmd = cargo();
    cmd.arg("llvm-cov");
    if use_nextest {
        cmd.arg("nextest");
    }
    cmd.args([
        "--workspace",
        "--all-features",
        "--no-report",
        "--ignore-filename-regex",
        IGNORE_FILENAME_REGEX,
    ]);
    expect_status(
        cmd.status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov run: {e}"))?,
        if use_nextest {
            "cargo llvm-cov nextest"
        } else {
            "cargo llvm-cov"
        },
    )
}

fn emit_html(open: bool) -> Result<(), String> {
    let mut cmd = cargo();
    cmd.args([
        "llvm-cov",
        "report",
        "--html",
        "--ignore-filename-regex",
        IGNORE_FILENAME_REGEX,
    ]);
    if open {
        cmd.arg("--open");
    }
    expect_status(
        cmd.status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov report --html: {e}"))?,
        "cargo llvm-cov report --html",
    )
}

fn emit_lcov() -> Result<(), String> {
    expect_status(
        cargo()
            .args([
                "llvm-cov",
                "report",
                "--lcov",
                "--output-path",
                "target/llvm-cov/lcov.info",
                "--ignore-filename-regex",
                IGNORE_FILENAME_REGEX,
            ])
            .status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov report --lcov: {e}"))?,
        "cargo llvm-cov report --lcov",
    )
}

fn emit_summary() -> Result<(), String> {
    expect_status(
        cargo()
            .args([
                "llvm-cov",
                "report",
                "--summary-only",
                "--ignore-filename-regex",
                IGNORE_FILENAME_REGEX,
            ])
            .status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov report --summary-only: {e}"))?,
        "cargo llvm-cov report --summary-only",
    )
}

fn gate(flag: &str, min: u32) -> Result<(), String> {
    expect_status(
        cargo()
            .args([
                "llvm-cov",
                "report",
                flag,
                &min.to_string(),
                "--ignore-filename-regex",
                IGNORE_FILENAME_REGEX,
            ])
            .status()
            .map_err(|e| format!("failed to spawn cargo llvm-cov gate ({flag}): {e}"))?,
        "cargo llvm-cov gate",
    )
}

fn cargo() -> Command {
    Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

fn ensure_llvm_cov_installed() -> Result<(), String> {
    let status = cargo()
        .args(["llvm-cov", "--version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() => Ok(()),
        _ => Err("cargo-llvm-cov is not installed. Install with:\n  \
             cargo install cargo-llvm-cov --locked\n\
             (or, on systems without rustc-bootstrap, the precompiled\n  \
             binary release at https://github.com/taiki-e/cargo-llvm-cov)\n\
             rustup component `llvm-tools-preview` is also required;\n\
             rust-toolchain.toml already pins it."
            .to_owned()),
    }
}
