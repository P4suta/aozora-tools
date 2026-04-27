//! aozora-tools developer tooling.
//!
//! Today: `xtask samply lsp-burst <SECONDS>` — wraps
//! [`samply`](https://github.com/mstange/samply) around the criterion
//! `burst` bench so we can attribute the per-edit wall-time on
//! `bouten.afm` to specific functions.
//!
//! Mirrors the pattern used by the sibling `aozora-xtask` in the main
//! `aozora` repo — Rust binary instead of a shell script because we
//! want one entry point that
//!
//! 1. fails fast when `kernel.perf_event_paranoid` is too high (so
//!    samply doesn't silently record zero samples),
//! 2. rebuilds the bench binary with debug info so symbolication
//!    works,
//! 3. resolves the binary path the same way cargo does.
//!
//! Run with:
//!
//! ```text
//! cargo run -p aozora-tools-xtask -- samply lsp-burst 30
//! ```

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

const PERF_PARANOID_PATH: &str = "/proc/sys/kernel/perf_event_paranoid";
const PERF_PARANOID_MAX: i32 = 1;
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
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Cmd::Samply(args) => match args.target {
            SamplyTarget::LspBurst { seconds } => samply_lsp_burst(seconds),
        },
    };
    if let Err(err) = result {
        eprintln!("xtask: {err}");
        process::exit(1);
    }
}

fn samply_lsp_burst(seconds: u32) -> Result<(), String> {
    require_perf_paranoid()?;

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

    eprintln!();
    eprintln!(">>> done. inspect with:");
    eprintln!(
        "    samply load {}        # opens local Firefox-Profiler UI",
        out.display()
    );
    Ok(())
}

fn require_perf_paranoid() -> Result<(), String> {
    let raw = match fs::read_to_string(PERF_PARANOID_PATH) {
        Ok(s) => s,
        Err(e) => {
            return Err(format!(
                "cannot read {PERF_PARANOID_PATH}: {e}\n\
                 samply needs perf_event_open(2). Without this file we can't \
                 tell whether the kernel will allow it."
            ));
        }
    };
    let level: i32 = raw
        .trim()
        .parse()
        .map_err(|e| format!("failed to parse {PERF_PARANOID_PATH}={raw:?}: {e}"))?;
    if level > PERF_PARANOID_MAX {
        return Err(format!(
            "\n\
             🔒  perf_event_paranoid = {level} — samply CANNOT collect samples here.\n\
             \n\
             ▸ One-shot fix (resets at next reboot):\n     \
                 echo {PERF_PARANOID_MAX} | sudo tee {PERF_PARANOID_PATH}\n\
             \n\
             ▸ Permanent fix (survives reboots):\n     \
                 echo 'kernel.perf_event_paranoid = {PERF_PARANOID_MAX}' | sudo tee /etc/sysctl.d/99-perf.conf\n     \
                 sudo sysctl --system\n\
             \n\
             samply uses perf_event_open(2) to sample the CPU at {SAMPLY_RATE_HZ}Hz; the\n\
             kernel guards that syscall behind perf_event_paranoid; the default of 2\n\
             blocks all unprivileged use, so samply would otherwise spawn but record\n\
             zero samples — silent and confusing.\n"
        ));
    }
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
