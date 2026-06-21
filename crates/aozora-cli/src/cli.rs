//! The `aozora` umbrella command tree.
//!
//! One `Cli` ([`Parser`]) with a `command` subcommand enum. The `fmt`/`lsp`
//! arms reuse the existing crates' argument structs so there is one definition
//! per surface; `lint`/`render`/`explain`/`completions`/`man` are defined here.

use std::path::PathBuf;

use clap::builder::Styles;
use clap::{Args, Parser, Subcommand, ValueEnum};

use aozora_fmt::ColorChoice;

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

const LONG_ABOUT: &str = "\
Authoring tools for aozora-flavored-markdown (青空文庫 notation).

Run `aozora <command> --help` for details. Common commands:
  aozora fmt      canonicalise documents (idempotent formatter)
  aozora lint     report diagnostics in the terminal
  aozora render   render a document to HTML
  aozora explain  explain a diagnostic code
  aozora lsp      run the language server (for editors)";

const AFTER_HELP: &str = "\
Examples:
  aozora fmt -w chapter.afm                 format a file in place
  aozora lint samples/                      lint a directory
  aozora lint --json doc.afm | jq           machine-readable diagnostics
  aozora explain aozora::unclosed-bracket   explain a diagnostic code
  aozora render doc.afm > doc.html          render to HTML";

const LSP_LONG_ABOUT: &str = "\
Run the aozora language server over stdio (JSON-RPC), for editor integration.

Environment:
  RUST_LOG                   tracing filter for stderr logs (default: warn)
  AOZORA_LSP_SLOW_PARSE_US   slow-parse warning threshold in µs (default: 100000)

`--stdio` is accepted (and ignored) for editor compatibility; the server
always speaks stdio.";

/// clap colour styling: cyan literals/placeholders, green headers, red errors —
/// matching the formatter's diff palette. Only takes effect with clap's `color`
/// feature (on by default).
fn styles() -> Styles {
    use anstyle::{AnsiColor, Style};
    Styles::styled()
        .header(Style::new().bold().fg_color(Some(AnsiColor::Green.into())))
        .usage(Style::new().bold().fg_color(Some(AnsiColor::Green.into())))
        .literal(Style::new().bold().fg_color(Some(AnsiColor::Cyan.into())))
        .placeholder(Style::new().fg_color(Some(AnsiColor::Cyan.into())))
        .valid(Style::new().fg_color(Some(AnsiColor::Green.into())))
        .invalid(Style::new().bold().fg_color(Some(AnsiColor::Red.into())))
        .error(Style::new().bold().fg_color(Some(AnsiColor::Red.into())))
}

/// The `aozora` command-line tool.
#[derive(Parser, Debug)]
#[command(
    name = "aozora",
    bin_name = "aozora",
    about = "Authoring tools for aozora-flavored-markdown",
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP,
    after_long_help = AFTER_HELP,
    version = LONG_VERSION,
    propagate_version = true,
    subcommand_required = true,
    arg_required_else_help = true,
    styles = styles(),
)]
pub struct Cli {
    /// Which tool to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The `aozora` subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Format aozora documents (idempotent canonicaliser).
    Fmt(aozora_fmt::FmtArgs),

    /// Lint aozora documents and print diagnostics to the terminal.
    #[command(visible_alias = "check")]
    Lint(LintArgs),

    /// Render an aozora document to HTML.
    Render(RenderArgs),

    /// Explain a diagnostic code (e.g. `aozora::unclosed-bracket`).
    Explain(ExplainArgs),

    /// Run the language server over stdio (for editors).
    #[command(long_about = LSP_LONG_ABOUT)]
    Lsp(LspArgs),

    /// Print a shell completion script to stdout.
    Completions(CompletionsArgs),

    /// Print a man page (troff) to stdout.
    Man(ManArgs),
}

/// `aozora lint` — report diagnostics in the terminal.
#[derive(Args, Debug)]
#[command(after_help = "\
Exit codes: 0 = clean, 1 = diagnostics found, 2 = error.
Warnings alone exit 0 unless --error-on-warning is given.

Examples:
  aozora lint samples/diagnostics.afm
  aozora lint --json doc.afm | jq
  aozora lint --watch chapter.afm")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "a clap flag struct: each bool is an independent CLI switch"
)]
pub struct LintArgs {
    /// Files or directories to lint. Use `-`, or omit, to read stdin.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Emit machine-readable JSON instead of rendered diagnostics.
    #[arg(long, conflicts_with_all = ["quiet", "watch"])]
    pub json: bool,

    /// Print one terse line per diagnostic (path:line:col: sev[code]: message).
    #[arg(long, short = 'q')]
    pub quiet: bool,

    /// Treat warnings as errors for the exit code.
    #[arg(long, short = 'W')]
    pub error_on_warning: bool,

    /// Re-run on every file change (clears the screen, like a dev server).
    #[arg(long)]
    pub watch: bool,

    /// Print a one-line summary (files, diagnostics, elapsed) to stderr.
    #[arg(long)]
    pub stats: bool,

    /// When to colourise output.
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,
}

/// `aozora render` — render a document to HTML.
#[derive(Args, Debug)]
#[command(after_help = "\
Examples:
  aozora render doc.afm > doc.html
  aozora render --standalone -o preview.html doc.afm
  aozora render --open doc.afm")]
pub struct RenderArgs {
    /// The file to render. Use `-`, or omit, to read stdin.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Write HTML to FILE instead of stdout.
    #[arg(long, short = 'o', value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Wrap the fragment in a standalone HTML5 document (vertical-writing CSS).
    #[arg(long)]
    pub standalone: bool,

    /// Open the rendered HTML in the default browser (implies --standalone).
    #[arg(long)]
    pub open: bool,

    /// Print render stats (bytes in/out, elapsed) to stderr.
    #[arg(long)]
    pub stats: bool,

    /// When to colourise error output.
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,
}

/// `aozora explain` — explain a diagnostic code.
#[derive(Args, Debug)]
pub struct ExplainArgs {
    /// The diagnostic code (e.g. `aozora::unclosed-bracket`, or just
    /// `unclosed-bracket`). Omit to list every code.
    #[arg(value_name = "CODE")]
    pub code: Option<String>,

    /// When to colourise output.
    #[arg(long, value_name = "WHEN", default_value = "auto")]
    pub color: ColorChoice,
}

/// `aozora lsp` — run the language server.
#[derive(Args, Debug, Clone, Copy)]
pub struct LspArgs {
    /// Accepted for editor compatibility; the server always speaks stdio.
    #[arg(long)]
    pub stdio: bool,
}

/// `aozora completions` — print a shell completion script.
#[derive(Args, Debug, Clone, Copy)]
pub struct CompletionsArgs {
    /// The shell to generate a completion script for.
    #[arg(value_name = "SHELL")]
    pub shell: CompletionShell,
}

/// Shells `aozora completions` can target (superset of `clap_complete::Shell`
/// adding Nushell).
#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum CompletionShell {
    /// Bash.
    Bash,
    /// Zsh.
    Zsh,
    /// Fish.
    Fish,
    /// PowerShell.
    Powershell,
    /// Nushell.
    Nushell,
}

/// `aozora man` — print a man page.
#[derive(Args, Debug)]
pub struct ManArgs {
    /// Render the page for this subcommand (e.g. `lint`). Omit for the
    /// top-level `aozora` page.
    #[arg(value_name = "SUBCOMMAND")]
    pub command: Option<String>,
}
