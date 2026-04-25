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
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
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
    let formatted = format_source(&source);
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
            fs::write(&path, &formatted).with_context(|| format!("writing {}", path.display()))?;
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Default: pipe the canonical form to stdout.
    io::stdout().write_all(formatted.as_bytes())?;
    Ok(ExitCode::SUCCESS)
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
