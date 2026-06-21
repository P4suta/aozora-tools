//! `aozora lint` — surface the diagnostic engine in the terminal.
//!
//! Reuses the formatter's path discovery ([`aozora_fmt::resolve`]) and panic
//! guard ([`aozora_fmt::guard`]); renders diagnostics with [`crate::render_term`]
//! (human) or a terse one-liner (`--quiet`); or emits a JSON report.

use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result};
use serde::Serialize;

use aozora_diagnostics::{Described, Severity, describe_source};
use aozora_fmt::{Input, auto_stdout, guard, resolve};

use crate::cli::LintArgs;
use crate::render_term::{self, TermDiag, line_col};
use crate::stats::{LintStats, LintStatsJson};
use crate::watch;

/// Exit-code-bearing result of a lint run: clean (0), diagnostics (1), error (2).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Outcome {
    /// No findings, every input read cleanly.
    Clean,
    /// At least one finding that should fail the run (error, or warning under
    /// `--error-on-warning`).
    Findings,
    /// An input could not be read, or the parser panicked.
    Error,
}

impl Outcome {
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Clean => ExitCode::SUCCESS,
            Self::Findings => ExitCode::from(1),
            Self::Error => ExitCode::from(2),
        }
    }
}

/// One input: a label plus either its content or a read-error message.
struct Source {
    label: String,
    content: Result<String, String>,
}

/// All inputs plus any non-fatal discovery (traversal) errors.
struct Collected {
    sources: Vec<Source>,
    discovery_errors: Vec<String>,
}

/// Run `aozora lint` and return the process exit code.
#[must_use]
pub(crate) fn run(args: &LintArgs) -> ExitCode {
    if args.watch {
        return watch::run(args);
    }
    match lint_paths(args) {
        Ok(outcome) => outcome.exit_code(),
        Err(err) => {
            eprintln!("aozora lint: {err:#}");
            ExitCode::from(2)
        }
    }
}

/// Lint the configured paths once. Reusable: `--watch` calls this per change.
pub(crate) fn lint_paths(args: &LintArgs) -> Result<Outcome> {
    let start = Instant::now();
    let collected = collect_sources(&args.paths)?;
    let mut stats = LintStats::default();

    let outcome = if args.json {
        emit_json(&collected, args, &mut stats, start)?
    } else {
        let outcome = render_stream(&collected, args, &mut stats)?;
        if args.stats {
            eprintln!("{}", stats.summary(start.elapsed()));
        }
        outcome
    };
    Ok(outcome)
}

/// Read every input (recursing directories) into memory.
fn collect_sources(paths: &[PathBuf]) -> Result<Collected> {
    match resolve(paths)? {
        Input::Stdin => {
            let mut content = String::new();
            io::stdin()
                .read_to_string(&mut content)
                .context("reading stdin")?;
            Ok(Collected {
                sources: vec![Source {
                    label: "<stdin>".to_owned(),
                    content: Ok(content),
                }],
                discovery_errors: Vec::new(),
            })
        }
        Input::Files(resolved) => {
            let sources = resolved
                .files
                .iter()
                .map(|path| Source {
                    label: path.display().to_string(),
                    content: fs::read_to_string(path).map_err(|e| e.to_string()),
                })
                .collect();
            Ok(Collected {
                sources,
                discovery_errors: resolved.errors,
            })
        }
    }
}

/// Parse `content` under the panic guard; `Err(())` means the parser panicked.
fn safe_describe(content: &str) -> Result<Vec<Described>, ()> {
    guard(|| describe_source(content)).map_err(|_| ())
}

/// Which severities a source produced, accumulated across its diagnostics.
#[derive(Default, Clone, Copy)]
struct Seen {
    error: bool,
    warning: bool,
}

impl Seen {
    /// The run outcome these severities imply, given `--error-on-warning`.
    fn outcome(self, error_on_warning: bool) -> Outcome {
        if self.error || (self.warning && error_on_warning) {
            Outcome::Findings
        } else {
            Outcome::Clean
        }
    }
}

// ---- streamed (human / --quiet) rendering ----

fn render_stream(collected: &Collected, args: &LintArgs, stats: &mut LintStats) -> Result<Outcome> {
    let mut outcome = Outcome::Clean;
    for err in &collected.discovery_errors {
        eprintln!("aozora lint: {err}");
        outcome = Outcome::Error;
    }
    let mut out = auto_stdout(args.color);
    for source in &collected.sources {
        outcome = outcome.max(lint_one_stream(source, args, stats, &mut out)?);
    }
    out.flush()?;
    Ok(outcome)
}

fn lint_one_stream(
    source: &Source,
    args: &LintArgs,
    stats: &mut LintStats,
    out: &mut impl Write,
) -> Result<Outcome> {
    stats.files_scanned += 1;
    let content = match &source.content {
        Ok(content) => content,
        Err(msg) => {
            stats.errored += 1;
            eprintln!("aozora lint: {}: {msg}", source.label);
            return Ok(Outcome::Error);
        }
    };
    let Ok(diags) = safe_describe(content) else {
        stats.errored += 1;
        eprintln!("aozora lint: {}: the parser panicked", source.label);
        return Ok(Outcome::Error);
    };
    if diags.is_empty() {
        stats.clean += 1;
        return Ok(Outcome::Clean);
    }
    stats.with_diagnostics += 1;
    let mut seen = Seen::default();
    for diag in &diags {
        tally(diag.severity, stats, &mut seen);
        let term = TermDiag {
            label: &source.label,
            source: content,
            diag,
        };
        if args.quiet {
            render_term::render_quiet(out, &term)?;
        } else {
            render_term::render(out, &term)?;
        }
    }
    Ok(seen.outcome(args.error_on_warning))
}

/// Count one diagnostic's severity into `stats` and the run-level [`Seen`] flags.
fn tally(severity: Severity, stats: &mut LintStats, seen: &mut Seen) {
    match severity {
        Severity::Error => {
            stats.errors += 1;
            seen.error = true;
        }
        Severity::Warning => {
            stats.warnings += 1;
            seen.warning = true;
        }
    }
}

// ---- JSON rendering ----

#[derive(Serialize)]
struct JsonReport<'a> {
    version: u32,
    ok: bool,
    files: Vec<JsonFile<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stats: Option<LintStatsJson>,
}

#[derive(Serialize)]
struct JsonFile<'a> {
    path: &'a str,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    diagnostics: Vec<JsonDiag>,
}

#[derive(Serialize)]
struct JsonDiag {
    code: &'static str,
    severity: Severity,
    message: String,
    span: JsonSpan,
    start: JsonPos,
    end: JsonPos,
}

#[derive(Serialize)]
struct JsonSpan {
    byte_start: u32,
    byte_end: u32,
}

#[derive(Serialize)]
struct JsonPos {
    line: usize,
    column: usize,
}

fn emit_json(
    collected: &Collected,
    args: &LintArgs,
    stats: &mut LintStats,
    start: Instant,
) -> Result<Outcome> {
    let mut files = Vec::new();
    let mut outcome = Outcome::Clean;
    for err in &collected.discovery_errors {
        files.push(JsonFile {
            path: "<discovery>",
            status: "error",
            message: Some(err),
            diagnostics: Vec::new(),
        });
        outcome = Outcome::Error;
    }
    for source in &collected.sources {
        outcome = outcome.max(json_one(source, args, stats, &mut files));
    }
    let report = JsonReport {
        version: 1,
        ok: outcome == Outcome::Clean,
        files,
        stats: args.stats.then(|| stats.to_json(start.elapsed())),
    };
    let mut out = io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, &report)?;
    out.write_all(b"\n")?;
    Ok(outcome)
}

fn json_one<'a>(
    source: &'a Source,
    args: &LintArgs,
    stats: &mut LintStats,
    files: &mut Vec<JsonFile<'a>>,
) -> Outcome {
    stats.files_scanned += 1;
    let content = match &source.content {
        Ok(content) => content,
        Err(msg) => {
            stats.errored += 1;
            files.push(JsonFile {
                path: &source.label,
                status: "error",
                message: Some(msg),
                diagnostics: Vec::new(),
            });
            return Outcome::Error;
        }
    };
    let Ok(diags) = safe_describe(content) else {
        stats.errored += 1;
        files.push(JsonFile {
            path: &source.label,
            status: "error",
            message: Some("the parser panicked"),
            diagnostics: Vec::new(),
        });
        return Outcome::Error;
    };
    if diags.is_empty() {
        stats.clean += 1;
        files.push(JsonFile {
            path: &source.label,
            status: "ok",
            message: None,
            diagnostics: Vec::new(),
        });
        return Outcome::Clean;
    }
    stats.with_diagnostics += 1;
    let mut seen = Seen::default();
    let diagnostics = diags
        .iter()
        .map(|diag| {
            tally(diag.severity, stats, &mut seen);
            json_diag(content, diag)
        })
        .collect();
    files.push(JsonFile {
        path: &source.label,
        status: "diagnostics",
        message: None,
        diagnostics,
    });
    seen.outcome(args.error_on_warning)
}

fn json_diag(content: &str, diag: &Described) -> JsonDiag {
    let (start_line, start_col) = line_col(content, diag.span.start as usize);
    let (end_line, end_col) = line_col(content, diag.span.end as usize);
    JsonDiag {
        code: diag.code,
        severity: diag.severity,
        message: diag.message.clone(),
        span: JsonSpan {
            byte_start: diag.span.start,
            byte_end: diag.span.end,
        },
        start: JsonPos {
            line: start_line,
            column: start_col,
        },
        end: JsonPos {
            line: end_line,
            column: end_col,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU32, Ordering};
    use std::{env, fs, process};

    fn args_for(paths: &[&str]) -> LintArgs {
        LintArgs {
            paths: paths.iter().map(PathBuf::from).collect(),
            json: false,
            quiet: false,
            error_on_warning: false,
            watch: false,
            stats: false,
            color: aozora_fmt::ColorChoice::Never,
        }
    }

    /// A fresh scratch dir with a clean and a broken sample, for the file-walk
    /// path of `collect_sources` / `lint_paths`.
    fn scratch_with_samples() -> PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let mut dir = env::temp_dir();
        dir.push(format!("aozora-lint-test-{}-{n}", process::id()));
        fs::create_dir_all(&dir).expect("scratch dir");
        fs::write(dir.join("clean.afm"), "｜日本《にほん》").expect("clean");
        fs::write(dir.join("bad.afm"), "本文［＃改ページ").expect("bad");
        dir
    }

    #[test]
    fn outcome_orders_by_severity() {
        assert!(Outcome::Clean < Outcome::Findings);
        assert!(Outcome::Findings < Outcome::Error);
    }

    #[test]
    fn seen_outcome_respects_error_on_warning() {
        let err = Seen {
            error: true,
            warning: false,
        };
        let warn = Seen {
            error: false,
            warning: true,
        };
        let none = Seen::default();
        assert_eq!(err.outcome(false), Outcome::Findings);
        assert_eq!(warn.outcome(false), Outcome::Clean);
        assert_eq!(warn.outcome(true), Outcome::Findings);
        assert_eq!(none.outcome(true), Outcome::Clean);
    }

    #[test]
    fn clean_source_streams_nothing_and_is_clean() {
        let source = Source {
            label: "ok.afm".to_owned(),
            content: Ok("｜日本《にほん》".to_owned()),
        };
        let mut stats = LintStats::default();
        let mut out = Vec::new();
        let outcome = lint_one_stream(&source, &args_for(&[]), &mut stats, &mut out).unwrap();
        assert_eq!(outcome, Outcome::Clean);
        assert_eq!(stats.clean, 1);
        assert!(out.is_empty(), "clean input prints nothing");
    }

    #[test]
    fn error_diagnostic_is_findings_and_counted() {
        let source = Source {
            label: "bad.afm".to_owned(),
            content: Ok("本文［＃改ページ".to_owned()),
        };
        let mut stats = LintStats::default();
        let mut out = Vec::new();
        let outcome = lint_one_stream(&source, &args_for(&[]), &mut stats, &mut out).unwrap();
        assert_eq!(outcome, Outcome::Findings);
        assert_eq!(stats.with_diagnostics, 1);
        assert_eq!(stats.errors, 1);
    }

    #[test]
    fn read_error_source_is_error_outcome() {
        let source = Source {
            label: "missing.afm".to_owned(),
            content: Err("No such file".to_owned()),
        };
        let mut stats = LintStats::default();
        let mut out = Vec::new();
        let outcome = lint_one_stream(&source, &args_for(&[]), &mut stats, &mut out).unwrap();
        assert_eq!(outcome, Outcome::Error);
        assert_eq!(stats.errored, 1);
    }

    #[test]
    fn lint_paths_over_a_directory_streams_and_counts() {
        let dir = scratch_with_samples();
        let mut args = args_for(&[]);
        args.paths = vec![dir.clone()];
        args.stats = true;
        let outcome = lint_paths(&args).expect("lint dir");
        assert_eq!(outcome, Outcome::Findings, "the broken file is a finding");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lint_paths_json_over_a_directory() {
        let dir = scratch_with_samples();
        let mut args = args_for(&[]);
        args.paths = vec![dir.clone()];
        args.json = true;
        args.stats = true;
        let outcome = lint_paths(&args).expect("lint dir json");
        assert_eq!(outcome, Outcome::Findings);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lint_paths_quiet_clean_directory_is_clean() {
        let dir = scratch_with_samples();
        fs::remove_file(dir.join("bad.afm")).ok();
        let mut args = args_for(&[]);
        args.paths = vec![dir.clone()];
        args.quiet = true;
        let outcome = lint_paths(&args).expect("lint clean dir");
        assert_eq!(outcome, Outcome::Clean);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn json_one_builds_diagnostics_entry() {
        let source = Source {
            label: "bad.afm".to_owned(),
            content: Ok("本文［＃改ページ".to_owned()),
        };
        let mut stats = LintStats::default();
        let mut files = Vec::new();
        let outcome = json_one(&source, &args_for(&[]), &mut stats, &mut files);
        assert_eq!(outcome, Outcome::Findings);
        assert_eq!(files.len(), 1);
        let value = serde_json::to_value(&files[0]).unwrap();
        assert_eq!(value["status"], "diagnostics");
        assert_eq!(value["diagnostics"][0]["code"], "aozora::unclosed-bracket");
        assert_eq!(value["diagnostics"][0]["severity"], "error");
        assert_eq!(value["diagnostics"][0]["start"]["column"], 3);
    }
}
