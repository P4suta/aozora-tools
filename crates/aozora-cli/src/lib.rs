//! The `aozora` umbrella CLI: `fmt`, `lint`, `render`, `explain`, and `lsp`
//! under one binary.
//!
//! `main.rs` is a thin shim that parses argv and calls [`run`]. Each subcommand
//! lives in its own module; `fmt`/`lsp` reuse the `aozora-fmt`/`aozora-lsp`
//! crates so there is one implementation per surface.

#![forbid(unsafe_code)]

mod assets;
mod cli;
mod explain;
mod lint;
mod render;
mod render_term;
mod stats;
mod watch;

use std::process::ExitCode;

use tokio::runtime::Builder;

pub use cli::Cli;
use cli::{Command, LspArgs};

/// Dispatch the parsed [`Cli`] to its subcommand and return the process exit
/// code.
#[must_use]
pub fn run(cli: Cli) -> ExitCode {
    match cli.command {
        Command::Fmt(args) => aozora_fmt::run_args(&args),
        Command::Lint(args) => lint::run(&args),
        Command::Render(args) => render::run(&args),
        Command::Explain(args) => explain::run(&args),
        Command::Lsp(args) => run_lsp(args),
        Command::Completions(args) => assets::completions(args),
        Command::Man(args) => assets::man(&args),
    }
}

/// Serve the language server over stdio.
///
/// argv was already fully parsed by clap (so `aozora lsp --help`/`--version`
/// printed and exited before we reach here), keeping stdout clean for the
/// JSON-RPC stream. The work is delegated to [`aozora_lsp::serve`].
fn run_lsp(_args: LspArgs) -> ExitCode {
    let runtime = match Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("aozora lsp: failed to start the async runtime: {err}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(aozora_lsp::serve());
    ExitCode::SUCCESS
}
