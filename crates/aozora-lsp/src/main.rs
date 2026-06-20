//! `aozora-lsp` daemon entry point. Speaks LSP over stdio.
//!
//! Logging goes to stderr because stdout is reserved for the LSP
//! JSON-RPC wire protocol. Set `RUST_LOG=aozora_lsp=debug` (or similar)
//! to see tracing events; the default filter is `warn` so quiet
//! editor integrations stay quiet.

#![forbid(unsafe_code)]

use std::io;
use tokio::io::{stdin, stdout};
use tower_lsp::{LspService, Server};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // Handle argv before touching stdout: clap prints and exits for
    // `--version`/`--help` (and on a usage error) here, before the JSON-RPC
    // stream is opened, so nothing pollutes the protocol channel. `--stdio`
    // is accepted (and ignored) for editor compatibility.
    let _cli = aozora_lsp::Cli::parse_args();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    let stdin = stdin();
    let stdout = stdout();
    // Custom requests `aozora/renderHtml` and `aozora/gaijiSpans` are
    // wired here at LspService build-time because tower-lsp's
    // `LanguageServer` trait only covers spec-defined methods; custom
    // methods go on the builder.
    let (service, socket) = LspService::build(aozora_lsp::Backend::new)
        .custom_method("aozora/renderHtml", aozora_lsp::Backend::render_html)
        .custom_method("aozora/gaijiSpans", aozora_lsp::Backend::gaiji_spans)
        .finish();
    // tower-lsp's default concurrency cap is 4. After a didChange,
    // VS Code routinely fires 5+ concurrent requests (codeAction,
    // inlayHint, renderHtml, plus repeat codeActions for ranges
    // either side of the cursor). The 5th and beyond wait for one of
    // the first four to finish — that surfaces as hundreds of
    // milliseconds of latency on otherwise µs handlers.
    // 32 keeps every realistic burst inside the parallel window;
    // none of our handlers consume executor threads beyond their
    // own work, so the higher cap is essentially free.
    Server::new(stdin, stdout, socket)
        .concurrency_level(32)
        .serve(service)
        .await;
}
