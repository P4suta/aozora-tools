//! tower-lsp `LanguageServer` implementation for aozora documents.
//!
//! State is a single [`DashMap<Url, String>`] holding the latest full
//! text of every open document. Every `did_open` / `did_change` event
//! overwrites the entry and re-publishes diagnostics; `did_close`
//! drops the entry and tells the client to clear diagnostics.
//!
//! The editor / parser interaction is synchronous from the LSP side
//! (we hold the lock for the duration of a parse), which is fine
//! because the parse + serialize pipeline is `O(n)` and sub-
//! millisecond for any realistic aozora buffer.

use std::sync::Arc;

use dashmap::DashMap;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, InitializedParams, MessageType, OneOf, ServerCapabilities, ServerInfo,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::{compute_diagnostics, format_edits, hover_at};

/// LSP backend for aozora documents.
#[derive(Debug)]
pub struct Backend {
    client: Client,
    docs: Arc<DashMap<Url, String>>,
}

impl Backend {
    /// Build a new backend. Signature matches `LspService::new`'s
    /// `FnOnce(Client) -> Backend` requirement, so users call this
    /// as `LspService::new(Backend::new)`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: Arc::new(DashMap::new()),
        }
    }

    async fn publish(&self, uri: Url, text: &str) {
        let diags = compute_diagnostics(text);
        self.client.publish_diagnostics(uri, diags, None).await;
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "aozora-lsp".to_owned(),
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "aozora-lsp ready")
            .await;
    }

    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        let uri = p.text_document.uri;
        let text = p.text_document.text;
        self.docs.insert(uri.clone(), text.clone());
        self.publish(uri, &text).await;
    }

    async fn did_change(&self, mut p: DidChangeTextDocumentParams) {
        let uri = p.text_document.uri;
        // TextDocumentSyncKind::FULL means exactly one content_changes
        // entry per notification, holding the whole new buffer.
        let Some(change) = p.content_changes.pop() else {
            return;
        };
        self.docs.insert(uri.clone(), change.text.clone());
        self.publish(uri, &change.text).await;
    }

    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        let uri = p.text_document.uri;
        self.docs.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn formatting(
        &self,
        p: DocumentFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        let uri = p.text_document.uri;
        let Some(text) = self.docs.get(&uri).map(|e| e.value().clone()) else {
            return Ok(None);
        };
        Ok(Some(format_edits(&text)))
    }

    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let uri = p.text_document_position_params.text_document.uri;
        let position = p.text_document_position_params.position;
        let Some(text) = self.docs.get(&uri).map(|e| e.value().clone()) else {
            return Ok(None);
        };
        Ok(hover_at(&text, position))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}
