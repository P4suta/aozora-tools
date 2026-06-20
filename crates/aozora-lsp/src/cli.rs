//! Command-line surface for the `aozora-lsp` daemon: just enough argv
//! handling for `--version`, `--help`, and the conventional `--stdio` flag
//! editors pass. The server itself only ever speaks LSP over stdio.

use clap::Parser;

/// Crate version annotated with the pinned upstream `aozora` parser, e.g.
/// `0.1.3 (aozora a53c632 / v0.4.1)`. The rev/tag are baked in by `build.rs`.
const LONG_VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (aozora ",
    env!("AOZORA_REV"),
    " / ",
    env!("AOZORA_TAG"),
    ")"
);

const LONG_ABOUT: &str = concat!(
    "Language Server for aozora-flavored-markdown. Speaks LSP over stdio: ",
    "stdout carries the JSON-RPC wire protocol and logs go to stderr.\n\n",
    "Environment variables:\n",
    "  RUST_LOG                  tracing filter, e.g. `aozora_lsp=debug` ",
    "(default: warn).\n",
    "  AOZORA_LSP_SLOW_PARSE_US  per-parse latency in microseconds above which ",
    "a slow-parse warning is logged (default: 100000).",
);

/// `aozora-lsp` — Language Server for aozora-flavored-markdown.
#[derive(Parser, Debug)]
#[command(
    name = "aozora-lsp",
    about = "Language Server for aozora-flavored-markdown (speaks LSP over stdio)",
    long_about = LONG_ABOUT,
    version = LONG_VERSION
)]
#[allow(
    missing_copy_implementations,
    reason = "a clap parser handle, not a value type; deriving Copy would mislead and break once string-valued args are added"
)]
pub struct Cli {
    /// Speak LSP over stdio. Accepted for editor compatibility; this is the
    /// only supported transport, so the flag is a no-op.
    #[arg(long)]
    #[allow(
        dead_code,
        reason = "--stdio is accepted for editor compatibility but never read: the daemon only speaks stdio"
    )]
    stdio: bool,
}

impl Cli {
    /// Parse argv. clap prints and exits for `--help`/`--version` (and on a
    /// usage error) *before* this returns, so the JSON-RPC stdout channel is
    /// never touched when those flags are present.
    #[must_use]
    pub fn parse_args() -> Self {
        Self::parse()
    }
}
