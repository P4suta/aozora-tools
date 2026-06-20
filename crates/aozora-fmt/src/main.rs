//! `aozora-fmt` — CLI formatter for aozora-flavored-markdown documents.
//!
//! Three modes:
//!
//! * default (no flag) — read from a file (or stdin with `-`) and write
//!   the canonicalised form to stdout.
//! * `--check` — exit non-zero if the file is not already formatted
//!   (prints the path to stderr). Matches the behaviour of `rustfmt --check`
//!   / `prettier --check` so it plugs into CI without extra glue.
//! * `--write` / `-w` — rewrite the file in place (no-op when the file
//!   is already canonical).
//!
//! Exit codes:
//!
//! * `0` — success (or `--check` and the file is already formatted).
//! * `1` — `--check` mode and the file would be reformatted.
//! * `2` — any other error (I/O, argument misuse).

#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind, set_hook, take_hook};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, anyhow, bail};
use aozora_fmt::format_source;
use clap::Parser;

/// Formatter for aozora-flavored-markdown.
#[derive(Parser, Debug)]
#[command(
    name = "aozora-fmt",
    about = "Idempotent formatter for aozora-flavored-markdown",
    version
)]
struct Cli {
    /// File to format. Use `-` (or omit) to read from stdin.
    path: Option<PathBuf>,

    /// Verify the file is already formatted. Exit status 1 if not.
    #[arg(long, conflicts_with = "write")]
    check: bool,

    /// Rewrite the file in place.
    #[arg(long, short = 'w', conflicts_with = "check")]
    write: bool,
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("aozora-fmt: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: &Cli) -> Result<ExitCode> {
    let (source, source_path) = read_input(cli.path.as_deref())?;
    let formatted = format_guarded(&source)?;
    let changed = formatted != source;

    if cli.check {
        if changed {
            let label = source_path
                .as_deref()
                .map_or_else(|| "<stdin>".to_owned(), |p| p.display().to_string());
            eprintln!("aozora-fmt: {label} would be reformatted");
            return Ok(ExitCode::from(1));
        }
        return Ok(ExitCode::SUCCESS);
    }

    if cli.write {
        let Some(path) = source_path else {
            bail!("--write requires a file path, not stdin");
        };
        if changed {
            // `format_source` is contractually idempotent; refuse to write
            // if a second pass changes the output rather than corrupt the file.
            let reformatted = format_guarded(&formatted)?;
            if reformatted != formatted {
                bail!(
                    "refusing to overwrite {}: formatting is not idempotent for this \
                     input (a second pass changes the output). This is a bug — please \
                     report it. The file was left unchanged.",
                    path.display()
                );
            }
            fs::write(&path, &formatted).with_context(|| format!("writing {}", path.display()))?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Default: pipe the canonical form to stdout.
    io::stdout().write_all(formatted.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

/// Format `source`, converting an upstream parser panic into a clean
/// exit-2 error instead of a process abort (via `panic = "unwind"` and
/// `catch_unwind`). In `--write` mode this guarantees no file is touched
/// after a panic.
fn format_guarded(source: &str) -> Result<String> {
    // Silence the default hook so a caught panic doesn't also print
    // "thread 'main' panicked …"; we report it ourselves below.
    let prev_hook = take_hook();
    set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(|| format_source(source)));
    set_hook(prev_hook);
    result.map_err(|_| {
        anyhow!(
            "the formatter panicked while processing this input; no files were \
             modified. This is a bug — please report it at \
             https://github.com/P4suta/aozora-tools/issues"
        )
    })
}

fn read_input(path: Option<&Path>) -> Result<(String, Option<PathBuf>)> {
    let is_stdin = path.is_none_or(|p| p == Path::new("-"));
    if is_stdin {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .context("reading stdin")?;
        return Ok((buf, None));
    }
    let path = path.expect("stdin branch handled above");
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    Ok((text, Some(path.to_path_buf())))
}
