//! `aozora-lsp` daemon entry point. Speaks LSP over stdio.
//!
//! Logging goes to stderr because stdout is reserved for the LSP
//! JSON-RPC wire protocol. Set `RUST_LOG=aozora_lsp=debug` (or similar)
//! to see tracing events; the default filter is `warn` so quiet
//! editor integrations stay quiet.

#![forbid(unsafe_code)]

use tower_lsp::{LspService, Server};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")))
        .with_writer(std::io::stderr)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(aozora_lsp::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
