//! Cross-target VS Code extension `.vsix` builder.
//!
//! Drives the same recipe documented in `editors/vscode/README.md`
//! across every platform VS Code Marketplace recognises as an extension
//! target:
//!
//! ```text
//! ┌─────────────┬───────────────────────────────┬──────────────┐
//! │ vsce target │ rust target                   │ build tool   │
//! ├─────────────┼───────────────────────────────┼──────────────┤
//! │ linux-x64   │ x86_64-unknown-linux-gnu      │ cargo        │
//! │ linux-arm64 │ aarch64-unknown-linux-gnu     │ cross        │
//! │ alpine-x64  │ x86_64-unknown-linux-musl     │ cross        │
//! │ alpine-arm64│ aarch64-unknown-linux-musl    │ cross        │
//! │ darwin-x64  │ x86_64-apple-darwin           │ zigbuild     │
//! │ darwin-arm64│ aarch64-apple-darwin          │ zigbuild     │
//! │ win32-x64   │ x86_64-pc-windows-gnu         │ cross        │
//! │ win32-arm64 │ aarch64-pc-windows-gnullvm    │ zigbuild     │
//! └─────────────┴───────────────────────────────┴──────────────┘
//! ```
//!
//! Concurrency model: builds run under a bounded thread pool
//! (`--jobs`, default 1 — see CLI doc for empirical justification).
//! Packaging is *serial* because every target must briefly hold
//! `editors/vscode/server/aozora-lsp[.exe]` exclusively before vsce
//! reads it; the package step is fast (~2 s) so serialising it is
//! invisible against the build wall (~70–90 s/target).

#![allow(
    clippy::cast_precision_loss,
    reason = "vsix sizes (bytes) and elapsed seconds are converted to f64 only for human-readable summary printing — well under f64 mantissa headroom in practice."
)]

use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use crate::workspace_root;

#[derive(Clone, Copy, Debug)]
pub struct TargetSpec {
    /// VS Code Marketplace target identifier (`vsce package --target`).
    pub vsce: &'static str,
    /// Rust target triple (`cargo build --target`).
    pub rust: &'static str,
    pub tool: BuildTool,
    /// Whether the produced binary has a `.exe` suffix.
    pub windows: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildTool {
    /// Native `cargo build`. Only safe for the host's own
    /// architecture/libc combination.
    Cargo,
    /// `cross build` — Docker container with a target-specific sysroot.
    /// Covers Linux glibc/musl and the `*-windows-gnu` targets cleanly.
    Cross,
    /// `cargo zigbuild` — uses zig as the linker, brings its own libc
    /// surface. Used for Apple targets (no Apple SDK required for our
    /// dep graph) and `aarch64-pc-windows-gnullvm` (cross's default
    /// Docker image lacks the llvm-mingw arm64-windows toolchain).
    Zigbuild,
}

impl BuildTool {
    fn label(self) -> &'static str {
        match self {
            BuildTool::Cargo => "cargo",
            BuildTool::Cross => "cross",
            BuildTool::Zigbuild => "cargo zigbuild",
        }
    }
}

pub const TARGETS: &[TargetSpec] = &[
    TargetSpec {
        vsce: "linux-x64",
        rust: "x86_64-unknown-linux-gnu",
        tool: BuildTool::Cargo,
        windows: false,
    },
    TargetSpec {
        vsce: "linux-arm64",
        rust: "aarch64-unknown-linux-gnu",
        tool: BuildTool::Cross,
        windows: false,
    },
    TargetSpec {
        vsce: "alpine-x64",
        rust: "x86_64-unknown-linux-musl",
        tool: BuildTool::Cross,
        windows: false,
    },
    TargetSpec {
        vsce: "alpine-arm64",
        rust: "aarch64-unknown-linux-musl",
        tool: BuildTool::Cross,
        windows: false,
    },
    TargetSpec {
        vsce: "darwin-x64",
        rust: "x86_64-apple-darwin",
        tool: BuildTool::Zigbuild,
        windows: false,
    },
    TargetSpec {
        vsce: "darwin-arm64",
        rust: "aarch64-apple-darwin",
        tool: BuildTool::Zigbuild,
        windows: false,
    },
    TargetSpec {
        vsce: "win32-x64",
        rust: "x86_64-pc-windows-gnu",
        tool: BuildTool::Cross,
        windows: true,
    },
    TargetSpec {
        vsce: "win32-arm64",
        rust: "aarch64-pc-windows-gnullvm",
        tool: BuildTool::Zigbuild,
        windows: true,
    },
];

#[derive(Debug)]
struct Row {
    spec: TargetSpec,
    build_secs: f64,
    package_secs: f64,
    vsix_size_bytes: u64,
    error: Option<String>,
}

pub fn run_vsix_all(jobs: usize, only: Option<&str>) -> Result<(), String> {
    let root = workspace_root()?;
    let server_dir = root.join("editors/vscode/server");
    let dist_dir = root.join("editors/vscode/dist-vsix");
    std::fs::create_dir_all(&dist_dir)
        .map_err(|e| format!("create_dir_all {}: {e}", dist_dir.display()))?;

    let selected: Vec<TargetSpec> = match only {
        Some(name) => TARGETS.iter().copied().filter(|s| s.vsce == name).collect(),
        None => TARGETS.to_vec(),
    };
    if selected.is_empty() {
        return Err(format!(
            "no targets matched {:?}; valid: {}",
            only,
            TARGETS
                .iter()
                .map(|t| t.vsce)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let actual_jobs = jobs.max(1).min(selected.len());
    eprintln!(
        "xtask vsix-all: jobs={}  targets={}  ({})",
        actual_jobs,
        selected.len(),
        selected
            .iter()
            .map(|s| s.vsce)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let total_start = Instant::now();

    // Bundle the TypeScript extension once up-front. The output
    // (`editors/vscode/out/extension.js`) is target-independent — the
    // same JS bundle goes into every per-platform .vsix, only the
    // shipped `server/aozora-lsp` binary varies. Running esbuild here
    // (rather than asking the dev to remember `bun run compile`) makes
    // `cargo xtask vsix-all` self-sufficient from a fresh `git clone +
    // bun install`.
    bundle_extension_js(&root)?;

    let build_results = run_parallel_builds(actual_jobs, &selected, &root);

    // Serialise the package phase: each target stages its binary into
    // `editors/vscode/server/` then immediately runs vsce package.
    let mut rows: Vec<Row> = Vec::with_capacity(selected.len());
    // Iterate in the original target order so the summary table reads
    // top-to-bottom in the same order as the constant.
    for spec in &selected {
        let build_res = build_results
            .iter()
            .find(|(s, _)| s.vsce == spec.vsce)
            .map_or_else(
                || Err("internal: missing build result".into()),
                |(_, r)| r.clone(),
            );

        let mut row = Row {
            spec: *spec,
            build_secs: 0.0,
            package_secs: 0.0,
            vsix_size_bytes: 0,
            error: None,
        };
        match build_res {
            Ok(secs) => row.build_secs = secs,
            Err(e) => {
                row.error = Some(e);
                rows.push(row);
                continue;
            }
        }
        let pack_start = Instant::now();
        match run_package(spec, &root, &server_dir, &dist_dir) {
            Ok((path, size)) => {
                row.package_secs = pack_start.elapsed().as_secs_f64();
                row.vsix_size_bytes = size;
                eprintln!(
                    "[{}] packaged → {} ({:.0} KiB)",
                    spec.vsce,
                    path.file_name().and_then(|s| s.to_str()).unwrap_or("?"),
                    (size as f64) / 1024.0,
                );
            }
            Err(e) => {
                row.error = Some(format!("package: {e}"));
            }
        }
        rows.push(row);
    }

    print_summary(&rows, total_start.elapsed().as_secs_f64());

    if rows.iter().any(|r| r.error.is_some()) {
        Err("one or more targets failed; see table above".into())
    } else {
        Ok(())
    }
}

#[allow(clippy::type_complexity)]
fn run_parallel_builds(
    jobs: usize,
    selected: &[TargetSpec],
    root: &Path,
) -> Vec<(TargetSpec, Result<f64, String>)> {
    // Pop-from-back work queue so workers serve targets one at a time
    // until empty. This is a simple work-stealing approximation —
    // because the per-target compile time is roughly uniform (~70–90 s
    // each), strict load balancing buys little; the queue model is
    // chosen for code simplicity.
    let queue: Arc<Mutex<Vec<TargetSpec>>> = Arc::new(Mutex::new(selected.to_vec()));
    let results: Arc<Mutex<Vec<(TargetSpec, Result<f64, String>)>>> =
        Arc::new(Mutex::new(Vec::with_capacity(selected.len())));

    let workers: Vec<_> = (0..jobs)
        .map(|i| {
            let queue = Arc::clone(&queue);
            let results = Arc::clone(&results);
            let root = root.to_path_buf();
            thread::Builder::new()
                .name(format!("vsix-build-{i}"))
                .spawn(move || {
                    loop {
                        let target = match queue.lock() {
                            Ok(mut q) => q.pop(),
                            Err(_) => return,
                        };
                        let Some(spec) = target else {
                            return;
                        };
                        let started = Instant::now();
                        eprintln!(
                            "[{}] build start  ({}, {})",
                            spec.vsce,
                            spec.rust,
                            spec.tool.label()
                        );
                        let raw_res = run_build(&spec, &root);
                        let elapsed = started.elapsed().as_secs_f64();
                        // Carry the elapsed time through the failure
                        // path so the summary table doesn't show 0.0 s
                        // for a build that actually ran to completion
                        // and then errored at link / vsce time.
                        let res = match raw_res {
                            Ok(()) => Ok(elapsed),
                            Err(e) => Err(format!("after {elapsed:.1}s: {e}")),
                        };
                        match &res {
                            Ok(s) => eprintln!("[{}] build OK    {s:.1}s", spec.vsce),
                            Err(e) => eprintln!("[{}] build FAIL  {e}", spec.vsce),
                        }
                        if let Ok(mut r) = results.lock() {
                            r.push((spec, res));
                        }
                    }
                })
                .expect("spawn build worker")
        })
        .collect();

    for w in workers {
        let _ = w.join();
    }
    Arc::try_unwrap(results)
        .ok()
        .and_then(|m| m.into_inner().ok())
        .unwrap_or_default()
}

fn run_build(spec: &TargetSpec, root: &Path) -> Result<(), String> {
    let mut cmd = match spec.tool {
        BuildTool::Cargo => {
            let mut c = Command::new("cargo");
            c.arg("build");
            c
        }
        BuildTool::Cross => {
            let mut c = Command::new("cross");
            c.arg("build");
            c
        }
        BuildTool::Zigbuild => {
            // `cargo zigbuild` needs `zig` on PATH. On a mise-managed
            // host (CLAUDE.md baseline), zig is installed but the
            // current non-interactive shell may not have re-activated
            // since `mise use -g zig` ran, so the install path isn't
            // in PATH. Wrap the invocation in `mise exec` so it
            // resolves zig deterministically against the active mise
            // toolchain. CI runners use setup-zig instead and never
            // hit this path.
            let mut c = Command::new("mise");
            c.args(["exec", "--", "cargo", "zigbuild"]);
            c
        }
    };
    cmd.arg("--locked")
        .arg("--profile")
        .arg("dist")
        .arg("--target")
        .arg(spec.rust)
        .arg("--bin")
        .arg("aozora-lsp")
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn {}: {e}", spec.tool.label()))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let prefix = format!("[{}]", spec.vsce);
    let p_out = prefix.clone();
    let p_err = prefix.clone();
    let h_out = thread::spawn(move || stream_with_prefix(stdout, &p_out, false));
    let h_err = thread::spawn(move || stream_with_prefix(stderr, &p_err, true));
    let status = child.wait().map_err(|e| format!("wait: {e}"))?;
    let _ = h_out.join();
    let _ = h_err.join();
    if status.success() {
        Ok(())
    } else {
        Err(format!("exit {status}"))
    }
}

fn stream_with_prefix<R: Read + Send + 'static>(reader: R, prefix: &str, to_stderr: bool) {
    let buf = BufReader::new(reader);
    for line in buf.lines() {
        let Ok(line) = line else { return };
        if to_stderr {
            eprintln!("{prefix} {line}");
        } else {
            println!("{prefix} {line}");
        }
    }
}

/// Run `bun run compile` (esbuild bundle) inside the VS Code
/// extension directory so `out/extension.js` is fresh before any
/// vsce package step picks it up. Cheap (~1 s) and idempotent —
/// running it when nothing changed re-emits an identical bundle.
fn bundle_extension_js(root: &Path) -> Result<(), String> {
    let ext_dir = root.join("editors/vscode");
    eprintln!("[bundle] esbuild → out/extension.js");
    let status = Command::new("bun")
        .args(["run", "compile"])
        .current_dir(&ext_dir)
        .status()
        .map_err(|e| format!("spawn `bun run compile` in {}: {e}", ext_dir.display()))?;
    if !status.success() {
        return Err(format!("bun run compile exit {status}"));
    }
    Ok(())
}

/// Refresh `editors/vscode/server/` with this target's freshly-built
/// binary. Each call wipes the directory first so a stale binary from
/// a previous target can't end up in the next vsix; the serial
/// package phase ensures only one target uses `server_dir` at a time.
fn stage_server_binary(spec: &TargetSpec, root: &Path, server_dir: &Path) -> Result<(), String> {
    if server_dir.exists() {
        for entry in std::fs::read_dir(server_dir)
            .map_err(|e| format!("read_dir {}: {e}", server_dir.display()))?
        {
            let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
            let _ = std::fs::remove_file(entry.path());
        }
    } else {
        std::fs::create_dir_all(server_dir)
            .map_err(|e| format!("create_dir_all {}: {e}", server_dir.display()))?;
    }
    let exe = if spec.windows {
        "aozora-lsp.exe"
    } else {
        "aozora-lsp"
    };
    let src = root.join("target").join(spec.rust).join("dist").join(exe);
    let dst = server_dir.join(exe);
    std::fs::copy(&src, &dst)
        .map_err(|e| format!("copy {} → {}: {e}", src.display(), dst.display()))?;
    if !spec.windows {
        // Stamp the executable bit explicitly. vsce's zip path happens
        // to preserve Unix mode bits today, but this documents intent
        // and is a no-op when already 0o755.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&dst) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(&dst, perms);
            }
        }
    }
    Ok(())
}

fn run_package(
    spec: &TargetSpec,
    root: &Path,
    server_dir: &Path,
    dist_dir: &Path,
) -> Result<(PathBuf, u64), String> {
    stage_server_binary(spec, root, server_dir)?;

    // `bunx @vscode/vsce package --target X --no-yarn` from inside
    // the extension directory. We capture but don't stream vsce
    // output — packaging is fast and noise-prone (one-line summary
    // is fine).
    let ext_dir = root.join("editors/vscode");
    let output = Command::new("bunx")
        .args([
            "@vscode/vsce",
            "package",
            "--target",
            spec.vsce,
            "--no-yarn",
        ])
        .current_dir(&ext_dir)
        .output()
        .map_err(|e| format!("spawn bunx vsce: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("vsce package exit {}: {stderr}", output.status));
    }

    // vsce drops `aozora-vscode-<vsce_target>-<version>.vsix` into the
    // extension dir. Find the most-recently-modified matching file
    // and move it into dist-vsix/ so the working tree stays clean.
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    for entry in
        std::fs::read_dir(&ext_dir).map_err(|e| format!("read_dir {}: {e}", ext_dir.display()))?
    {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !Path::new(name)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("vsix"))
            || !name.contains(spec.vsce)
        {
            continue;
        }
        let mtime = entry
            .metadata()
            .map_err(|e| format!("stat {name}: {e}"))?
            .modified()
            .unwrap_or(UNIX_EPOCH);
        if latest.as_ref().is_none_or(|(t, _)| mtime > *t) {
            latest = Some((mtime, path));
        }
    }
    let vsix = latest
        .ok_or_else(|| {
            format!(
                "no .vsix matching '{}' found in {}",
                spec.vsce,
                ext_dir.display()
            )
        })?
        .1;
    let final_dest = dist_dir.join(
        vsix.file_name()
            .ok_or_else(|| "vsix path has no file name".to_string())?,
    );
    std::fs::rename(&vsix, &final_dest)
        .map_err(|e| format!("rename {} → {}: {e}", vsix.display(), final_dest.display()))?;
    let size = std::fs::metadata(&final_dest)
        .map_err(|e| format!("stat {}: {e}", final_dest.display()))?
        .len();
    Ok((final_dest, size))
}

fn print_summary(rows: &[Row], total_secs: f64) {
    eprintln!();
    eprintln!(
        "xtask vsix-all: total wall {:.1}s ({} target{})",
        total_secs,
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    );
    eprintln!();
    eprintln!(
        "  {:<14}  {:>9}  {:>9}  {:>9}  status",
        "target", "build", "package", "size"
    );
    eprintln!(
        "  {:<14}  {:>9}  {:>9}  {:>9}  ------",
        "------", "------", "------", "------"
    );
    for row in rows {
        let size_kib = (row.vsix_size_bytes as f64) / 1024.0;
        let status = match &row.error {
            None => "OK".to_string(),
            Some(e) => format!("FAIL: {e}"),
        };
        eprintln!(
            "  {:<14}  {:>8.1}s  {:>8.1}s  {:>7.0}KiB  {}",
            row.spec.vsce, row.build_secs, row.package_secs, size_kib, status
        );
    }
    eprintln!();
}
