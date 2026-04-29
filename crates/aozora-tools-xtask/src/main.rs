//! aozora-tools developer tooling.
//!
//! Two `samply`-related subcommands today:
//!
//! - `xtask samply lsp-burst <SECONDS>` — runs the criterion `burst`
//!   bench under [samply](https://github.com/mstange/samply) so we
//!   can attribute the per-edit wall-time on `bouten.afm` to specific
//!   functions. Includes pre-flight environment checks so the trace
//!   isn't polluted by CPU governor throttling, memory pressure, or
//!   other-tenant CPU load.
//! - `xtask samply analyze <TRACE>` — post-processes a `.json.gz`
//!   trace into a CLI top-N "self-time per leaf function" report.
//!   Stdout is plain text so the report diffs cleanly between runs.
//!
//! Workflow + pre/post pipeline documented in `docs/profiling.md`.

#![forbid(unsafe_code)]
#![allow(
    clippy::print_stderr,
    clippy::exit,
    reason = "xtask binary uses std::process::exit / eprintln! to wire up the spawned `cargo` and `samply` invocations; both are appropriate here, in the dev-tooling crate, but disallowed elsewhere."
)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{self, Command},
};

use clap::{Args, Parser, Subcommand};

mod analyze;
mod preflight;
mod vsix;

const SAMPLY_RATE_HZ: u32 = 4000;
const DEFAULT_LSP_BURST_SECONDS: u32 = 30;

#[derive(Parser)]
#[command(name = "xtask", about = "aozora-tools developer tooling", version)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Sample-profile a target via `samply`.
    Samply(SamplyArgs),
    /// Build the bundled-LSP VS Code extension `.vsix` for every
    /// platform target VS Code Marketplace recognises (linux/alpine/
    /// darwin/win32 × x64/arm64). Builds run in parallel under a
    /// bounded thread pool; packaging is serial to avoid races on
    /// `editors/vscode/server/`. Outputs land in
    /// `editors/vscode/dist-vsix/`.
    VsixAll(VsixAllArgs),
}

#[derive(Args)]
struct VsixAllArgs {
    /// Maximum number of build jobs to run concurrently. Default 1
    /// (serial) is empirically the fastest setting on WSL2 — measured
    /// `--jobs 2` at 597 s vs serial 605 s (within noise) and
    /// `--jobs 4` actively regresses (Docker daemon contention plus
    /// IO bandwidth on the shared registry/git-cache routinely trips
    /// cross's container with exit 101). Raise on a true Linux host
    /// with ≥16 GiB free RAM and a dedicated Docker daemon while
    /// watching `docker stats`; values below 1 are clamped.
    #[arg(long, default_value_t = 1)]
    jobs: usize,

    /// Build only the named vsce target (e.g. `linux-x64`). Useful
    /// while iterating locally on the platform you actually run.
    /// Without this flag, all 8 targets are built.
    #[arg(long)]
    target: Option<String>,
}

#[derive(Args)]
struct SamplyArgs {
    #[command(subcommand)]
    target: SamplyTarget,
}

#[derive(Subcommand)]
enum SamplyTarget {
    /// Profile the LSP keystroke-burst hot path via the criterion
    /// `burst` bench under `aozora-lsp`.
    ///
    /// Builds the bench binary with `--profile=bench` (release with
    /// debug info preserved), then runs it under `samply record` with
    /// criterion's `--profile-time <SECONDS>` flag so each benchmark
    /// loops in a tight measurement window without setup/teardown
    /// dominating the trace.
    LspBurst {
        /// How many seconds criterion should spin each benchmark in
        /// profiling mode. 30 s is enough to amortise warm-up and
        /// give samply ~120 k samples at 4 kHz.
        #[arg(default_value_t = DEFAULT_LSP_BURST_SECONDS)]
        seconds: u32,
    },
    /// Post-process a captured trace into a top-N self-time report.
    ///
    /// Output is plain text on stdout — pipe to `tee` to save, or
    /// run twice (baseline + variant) and `diff` the outputs to see
    /// what moved. The Firefox-Profiler GUI is still available via
    /// `samply load <trace>` if you need the full call hierarchy.
    Analyze {
        /// Path to the `.json.gz` trace produced by an earlier
        /// `samply record` run.
        trace: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Samply(args) => match args.target {
            SamplyTarget::LspBurst { seconds } => samply_lsp_burst(seconds),
            SamplyTarget::Analyze { trace } => analyze::analyze(&trace),
        },
        Cmd::VsixAll(args) => vsix::run_vsix_all(args.jobs, args.target.as_deref()),
    };
    if let Err(err) = result {
        eprintln!("xtask: {err}");
        process::exit(1);
    }
}

fn samply_lsp_burst(seconds: u32) -> Result<(), String> {
    preflight::run_preflight(SAMPLY_RATE_HZ)?;

    let run_id = current_run_id();
    let out = PathBuf::from("/tmp").join(format!("aozora-lsp-burst-{run_id}.json.gz"));

    rebuild_bench_with_debug("burst", "aozora-lsp")?;
    let bin = bench_binary_path("aozora-lsp", "burst")?;

    eprintln!(
        ">>> samply: bench=burst seconds={seconds}\n           out={}",
        out.display()
    );
    // Pass `--profile-time` so criterion runs each benchmark in a tight
    // measurement loop without warmup/setup dominating the trace.
    let status = Command::new("samply")
        .arg("record")
        .arg("--save-only")
        .arg("--no-open")
        // Bakes symbol info into a .syms.json sidecar so the CLI
        // analyzer (and any downstream tooling) sees function names
        // instead of raw `0x…` addresses. Without this, samply
        // resolves symbols lazily when `samply load` is invoked,
        // which our CLI report path skips.
        .arg("--unstable-presymbolicate")
        .arg("-o")
        .arg(&out)
        .arg("-r")
        .arg(SAMPLY_RATE_HZ.to_string())
        .arg("--")
        .arg(bin)
        .arg("--bench")
        .arg("--profile-time")
        .arg(seconds.to_string())
        .status()
        .map_err(|e| format!("failed to spawn samply: {e}"))?;
    expect_status(status, "samply record")?;

    preflight::print_post_run_help(&out, SAMPLY_RATE_HZ);
    Ok(())
}

fn rebuild_bench_with_debug(bench: &str, package: &str) -> Result<(), String> {
    eprintln!(">>> rebuilding bench `{bench}` ({package}) with debug info (--profile=bench)");
    let status = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .arg("bench")
        .arg("--no-run")
        .arg("--bench")
        .arg(bench)
        .arg("-p")
        .arg(package)
        .status()
        .map_err(|e| format!("failed to spawn cargo: {e}"))?;
    expect_status(status, "cargo bench --no-run")
}

/// Resolve the on-disk bench binary path. Cargo emits criterion bench
/// outputs to `target/release/deps/<bench>-<hash>` and also writes a
/// stable convenience copy at `target/release/<bench>-<hash>` in some
/// versions. The hash is unstable, so we glob the latest matching file
/// in `deps/` instead of guessing.
fn bench_binary_path(_package: &str, bench: &str) -> Result<PathBuf, String> {
    let deps = workspace_root()?
        .join("target")
        .join("release")
        .join("deps");
    let prefix = format!("{bench}-");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(&deps).map_err(|e| format!("read_dir {}: {e}", deps.display()))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with(&prefix) {
            continue;
        }
        // Skip auxiliary artifacts (.d, .json, .pdb, .rmeta, …).
        if name.contains('.') {
            continue;
        }
        let path = entry.path();
        let meta = entry.metadata().map_err(|e| format!("stat {name}: {e}"))?;
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().map_err(|e| format!("mtime {name}: {e}"))?;
        if newest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            newest = Some((mtime, path));
        }
    }
    newest.map(|(_, p)| p).ok_or_else(|| {
        format!(
            "no bench binary `{bench}-*` under {} — did `cargo bench --no-run` succeed?",
            deps.display()
        )
    })
}

fn workspace_root() -> Result<PathBuf, String> {
    // CARGO_MANIFEST_DIR points at this xtask crate; go up two to
    // reach the workspace root (`crates/aozora-tools-xtask` → `..`).
    let manifest =
        env::var_os("CARGO_MANIFEST_DIR").ok_or_else(|| "CARGO_MANIFEST_DIR not set".to_owned())?;
    let manifest_path = Path::new(&manifest);
    let path = manifest_path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| {
            format!(
                "CARGO_MANIFEST_DIR={} has no grandparent",
                manifest_path.display()
            )
        })?
        .to_path_buf();
    Ok(path)
}

fn expect_status(status: process::ExitStatus, what: &str) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed with {status}"))
    }
}

/// Epoch-seconds + PID basename for trace files. Sortable, unique
/// across concurrent invocations, and avoids a date-math dependency
/// for what is just a "don't clobber prior runs" filename hint.
fn current_run_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let pid = process::id();
    format!("{secs}-{pid}")
}
