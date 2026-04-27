//! Pre-flight environment checks for `samply record`.
//!
//! Profiling on a noisy host produces noisy traces. The user reads
//! the resulting flame graph as if the numbers were authoritative,
//! and then chases optimisations against measurement variance
//! instead of real cost. The checks below catch the common
//! sources of noise BEFORE samply spawns, so the captured trace is
//! actually trustworthy:
//!
//! 1. `kernel.perf_event_paranoid` ≤ 1, else samply records zero
//!    samples (hard failure — abort).
//! 2. CPU governor on `performance` (warn-only — high-frequency
//!    samples on a `powersave` governor produce a misleading "this
//!    function takes longer" picture because the clock was throttled
//!    when sampling triggered).
//! 3. Available physical memory ≥ 1 GiB free (warn-only — disk
//!    pressure during sampling adds page-fault frames that aren't
//!    real per-edit cost).
//!
//! Each check is encapsulated so future additions (huge-page status,
//! turbo-boost state, ftrace contention) slot in cleanly. The main
//! entry point [`run_preflight`] runs all checks in order, prints a
//! summary, and returns `Err` only on hard-failure conditions —
//! warnings are surfaced but don't block.

use std::fs;

const PERF_PARANOID_PATH: &str = "/proc/sys/kernel/perf_event_paranoid";
const PERF_PARANOID_MAX: i32 = 1;
const SAMPLY_RATE_HZ_DEFAULT: u32 = 4000;

/// Outcome of a single check. `Ok` and `Warn` print a one-line
/// summary; `Hard` aborts the run.
#[derive(Debug)]
enum Check {
    Ok(String),
    Warn(String),
    Hard(String),
}

/// Run every pre-flight check in order. Hard-failures are returned
/// as `Err`; warnings are printed but don't block.
pub fn run_preflight(rate_hz: u32) -> Result<(), String> {
    eprintln!(">>> samply preflight (rate={rate_hz} Hz)");
    let checks = [
        ("perf_event_paranoid", check_perf_paranoid()),
        ("cpu governor", check_cpu_governor()),
        ("free memory", check_free_memory()),
        ("background CPU load", check_loadavg()),
    ];
    let mut hard: Option<String> = None;
    for (name, result) in checks {
        match result {
            Check::Ok(msg) => eprintln!("    ✓  {name}: {msg}"),
            Check::Warn(msg) => eprintln!("    ⚠  {name}: {msg}"),
            Check::Hard(msg) => {
                eprintln!("    ✗  {name}: {msg}");
                hard.get_or_insert(msg);
            }
        }
    }
    match hard {
        Some(msg) => Err(format!("preflight aborted: {msg}")),
        None => Ok(()),
    }
}

fn check_perf_paranoid() -> Check {
    let raw = match fs::read_to_string(PERF_PARANOID_PATH) {
        Ok(s) => s,
        Err(e) => {
            return Check::Hard(format!(
                "cannot read {PERF_PARANOID_PATH}: {e} (samply needs perf_event_open(2))"
            ));
        }
    };
    let level: i32 = match raw.trim().parse() {
        Ok(v) => v,
        Err(e) => return Check::Hard(format!("parse {PERF_PARANOID_PATH}={raw:?}: {e}")),
    };
    if level > PERF_PARANOID_MAX {
        return Check::Hard(format!(
            "= {level} (need ≤ {PERF_PARANOID_MAX}); fix: \
             `echo {PERF_PARANOID_MAX} | sudo tee {PERF_PARANOID_PATH}`"
        ));
    }
    Check::Ok(format!(
        "= {level} (≤ {PERF_PARANOID_MAX} → samply allowed)"
    ))
}

fn check_cpu_governor() -> Check {
    // /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor
    let path = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => return Check::Warn(format!("cannot read {path}: {e} (skipped)")),
    };
    let governor = raw.trim().to_owned();
    if governor == "performance" {
        Check::Ok(format!("cpu0 = {governor}"))
    } else {
        Check::Warn(format!(
            "cpu0 = {governor} (consider `performance` for stable samples; \
             fix: `echo performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor`)"
        ))
    }
}

fn check_free_memory() -> Check {
    let raw = match fs::read_to_string("/proc/meminfo") {
        Ok(s) => s,
        Err(e) => return Check::Warn(format!("cannot read /proc/meminfo: {e} (skipped)")),
    };
    let available_kb = raw
        .lines()
        .find(|l| l.starts_with("MemAvailable:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok());
    let Some(kb) = available_kb else {
        return Check::Warn("MemAvailable line missing in /proc/meminfo".to_owned());
    };
    let mib = kb / 1024;
    if mib < 1024 {
        Check::Warn(format!(
            "MemAvailable = {mib} MiB < 1 GiB \
             (page-faults under pressure pollute trace)"
        ))
    } else {
        Check::Ok(format!("MemAvailable = {mib} MiB"))
    }
}

fn check_loadavg() -> Check {
    let raw = match fs::read_to_string("/proc/loadavg") {
        Ok(s) => s,
        Err(e) => return Check::Warn(format!("cannot read /proc/loadavg: {e} (skipped)")),
    };
    let load_1m = raw
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f32>().ok());
    let cpus = u32::try_from(num_cpus_online()).unwrap_or(1).max(1);
    let cpus_f = f32::from(u16::try_from(cpus).unwrap_or(u16::MAX));
    let Some(load) = load_1m else {
        return Check::Warn("loadavg unparseable".to_owned());
    };
    let ratio = load / cpus_f;
    if ratio > 0.5 {
        Check::Warn(format!(
            "loadavg-1m = {load:.2} on {cpus_f:.0} CPUs (= {pct:.0}% utilised); \
             other CPU-heavy work will distort samples",
            pct = ratio * 100.0
        ))
    } else {
        Check::Ok(format!(
            "loadavg-1m = {load:.2} on {cpus_f:.0} CPUs ({pct:.0}% utilised)",
            pct = ratio * 100.0
        ))
    }
}

fn num_cpus_online() -> usize {
    // Cheap parse of /sys/devices/system/cpu/online ("0-15" form).
    let Ok(raw) = fs::read_to_string("/sys/devices/system/cpu/online") else {
        return 1;
    };
    raw.trim()
        .split(',')
        .map(|range| {
            let mut it = range.splitn(2, '-');
            let lo: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let hi: usize = it.next().and_then(|s| s.parse().ok()).unwrap_or(lo);
            hi - lo + 1
        })
        .sum()
}

/// Print a brief reminder of the post-capture workflow at the end
/// of a successful samply run. Centralised so the wording stays
/// identical across every `samply record` target.
pub fn print_post_run_help(out: &std::path::Path, rate_hz: u32) {
    let _ = rate_hz; // reserved for future "samples expected" estimate
    eprintln!();
    eprintln!(">>> samply trace captured");
    if let Ok(meta) = std::fs::metadata(out) {
        eprintln!("    file: {} ({} bytes)", out.display(), meta.len());
    }
    eprintln!();
    eprintln!(">>> next steps:");
    eprintln!(
        "    1. CLI top-N report:   cargo run -p aozora-tools-xtask -- samply analyze {}",
        out.display()
    );
    eprintln!("    2. Firefox Profiler:   samply load {}", out.display());
    let _ = SAMPLY_RATE_HZ_DEFAULT; // pin for clippy
}
