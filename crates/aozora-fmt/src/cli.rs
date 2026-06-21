//! Command-line surface for `aozora-fmt`: the clap [`Cli`] plus the [`Mode`]
//! it derives once so the rest of the pipeline never juggles raw booleans.
//!
//! `Cli` is re-exported from the crate root so `xtask` can reach
//! `Cli::command()` to generate shell completions and the man page.

use std::path::{Path, PathBuf};

use clap::{Parser, ValueEnum};

/// Crate version annotated with the pinned upstream `aozora` parser, e.g.
/// `0.4.1 (aozora a53c632 / v0.4.1)`. The rev/tag are baked in by `build.rs`.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (aozora ",
    env!("AOZORA_REV"),
    " / ",
    env!("AOZORA_TAG"),
    ")"
);

const LONG_ABOUT: &str = concat!(
    "Idempotent formatter for aozora-flavored-markdown.\n\n",
    "With no path (or `-`) it reads stdin and writes the canonical form to ",
    "stdout. Given files or directories it can check, rewrite, list, or diff ",
    "them; directories are searched recursively for *.afm, *.aozora, and ",
    "*.aozora.txt files.\n\n",
    "Exit codes: 0 = success / already formatted, 1 = --check found inputs ",
    "that would change, 2 = an error occurred.",
);

/// Idempotent formatter for aozora-flavored-markdown.
#[derive(Parser, Debug)]
#[command(
    name = "aozora-fmt",
    about = "Idempotent formatter for aozora-flavored-markdown",
    long_about = LONG_ABOUT,
    version = LONG_VERSION
)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a clap flag struct: each bool is an independent CLI switch, collapsed into Mode by mode()"
)]
pub struct Cli {
    /// Files or directories to format. Use `-`, or omit, to read stdin.
    #[arg(value_name = "PATH")]
    paths: Vec<PathBuf>,

    /// Verify inputs are already formatted; exit 1 if any would change.
    #[arg(long, conflicts_with_all = ["write", "list"])]
    check: bool,

    /// Rewrite files in place (no-op when already canonical).
    #[arg(long, short = 'w', conflicts_with_all = ["check", "diff", "json"])]
    write: bool,

    /// Print a unified diff for every file that would change. Implies --check.
    #[arg(long, conflicts_with_all = ["write", "list", "json"])]
    diff: bool,

    /// List only the paths that would change (gofmt -l). Combine with -w.
    #[arg(long, short = 'l', conflicts_with_all = ["check", "diff", "json"])]
    list: bool,

    /// Emit the --check result as machine-readable JSON. Implies --check.
    #[arg(long, conflicts_with_all = ["write", "list", "diff"])]
    json: bool,

    /// When to colourise --diff output.
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    color: ColorChoice,
}

/// When to emit ANSI colour in `--diff` output.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum ColorChoice {
    /// Colour when stdout is a terminal (honours `NO_COLOR`).
    Auto,
    /// Always colour, even when piped.
    Always,
    /// Never colour.
    Never,
}

/// What a run should do, derived once from the parsed flags.
pub(crate) enum Mode {
    /// Pipe the canonical form to stdout (valid for a single input only).
    Stdout,
    /// Rewrite changed files in place; `list` also prints them (gofmt -l -w).
    Write { list: bool },
    /// Verify formatting; the [`CheckReport`] controls what is printed.
    Check(CheckReport),
    /// Print only the paths that would change (gofmt -l).
    List,
}

/// How `--check` reports files that are not already formatted.
pub(crate) enum CheckReport {
    /// One `<path> would be reformatted` line per file, on stderr.
    Plain,
    /// A coloured unified diff per file, on stdout.
    Diff,
    /// A single JSON object on stdout.
    Json,
}

impl Cli {
    /// The positional path arguments (possibly including `-` for stdin).
    pub(crate) fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// The chosen colour policy for diff output.
    pub(crate) fn color(&self) -> ColorChoice {
        self.color
    }

    /// Collapse the mutually-exclusive flags into a single [`Mode`].
    pub(crate) fn mode(&self) -> Mode {
        if self.write {
            Mode::Write { list: self.list }
        } else if self.list {
            Mode::List
        } else if self.diff {
            Mode::Check(CheckReport::Diff)
        } else if self.json {
            Mode::Check(CheckReport::Json)
        } else if self.check {
            Mode::Check(CheckReport::Plain)
        } else {
            Mode::Stdout
        }
    }
}

/// True if `path` is the stdin sentinel `-`.
pub(crate) fn is_stdin(path: &Path) -> bool {
    path == Path::new("-")
}
