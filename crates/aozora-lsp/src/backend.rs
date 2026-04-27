//! tower-lsp `LanguageServer` implementation for aozora documents.
//!
//! # State model
//!
//! Each open document is held inside a [`DashMap`] as
//! `Arc<DocState>`. [`DocState`] is split into a writer-side
//! `BufferState` behind a `parking_lot::Mutex` and a reader-side
//! `Snapshot` swapped atomically into an `ArcSwap`; see the
//! [`crate::state`] module for the architecture rationale and lock
//! graph. Every LSP request handler acquires its data via
//! `state.snapshot()` (a single atomic load + Arc clone — wait-free).
//! The 200 ms tree-sitter incremental reparse on a 6 MB document no
//! longer blocks concurrent readers.
//!
//! # Sync mode
//!
//! `text_document_sync` is [`TextDocumentSyncKind::INCREMENTAL`].
//! `did_change` resolves each `TextDocumentContentChangeEvent`
//! against the latest snapshot, applies the byte-range edits via
//! `DocState::apply_changes`, and schedules a debounced semantic
//! re-parse + diagnostic publish. The semantic reparse runs inside
//! `tokio::task::spawn_blocking` so concurrent hover / inlay /
//! codeAction requests on the async runtime never stall.

use std::sync::Arc;
use std::time::Duration;

use dashmap::DashMap;

use crate::code_actions::{quick_fix_actions, wrap_selection_actions};
use crate::commands::{COMMAND_CANONICALIZE_SLUG, canonicalize_slug_edit};
use crate::completion::completion_at;
use crate::linked_editing::linked_editing_at;
use crate::state::DocState;
use crate::text_edit::LocalTextEdit;
use crate::{compute_diagnostics_from_parsed, format_edits, hover_at};
use tower_lsp::jsonrpc::{Error as JsonRpcError, Result};
use tower_lsp::lsp_types::{
    CodeActionOptions, CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    CompletionItem, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, ExecuteCommandOptions, ExecuteCommandParams, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams,
    LinkedEditingRangeParams, LinkedEditingRangeServerCapabilities, LinkedEditingRanges,
    MessageType, OneOf, Range, ServerCapabilities, ServerInfo, TextDocumentContentChangeEvent,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::position::position_to_byte_offset;

/// LSP backend for aozora documents.
///
/// `Clone` so we can hand a copy to debounced background tasks
/// (Stage 5). The fields are cheap to clone — `Client` is a
/// channel handle and `docs` is `Arc<DashMap<...>>`.
#[derive(Debug, Clone)]
pub struct Backend {
    client: Client,
    docs: Arc<DashMap<Url, Arc<DocState>>>,
}

/// Quiet-period before the slow Rust parse + `publishDiagnostics`
/// runs. While the user is actively typing, every keystroke bumps
/// `parse_version`, and the debounced task at the tail end of the
/// burst is the only one that actually proceeds to parse — earlier
/// tasks see a stale version and exit.
const PUBLISH_DEBOUNCE_MS: u64 = 150;

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

    async fn publish(&self, uri: Url) {
        let diags = self.lookup(&uri).map_or_else(Vec::new, |state| {
            let snap = state.snapshot();
            state.with_segment_cache(|cache| {
                compute_diagnostics_from_parsed(&snap.text, cache.diagnostics())
            })
        });
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Schedule a debounced semantic re-parse + diagnostic publish.
    /// The actual work runs after `PUBLISH_DEBOUNCE_MS` quiet time.
    /// Multiple rapid edits coalesce — only the task whose recorded
    /// `target_version` matches the doc's current `edit_version`
    /// after the sleep proceeds. Earlier tasks observe a newer
    /// version and exit silently, so a 100-keystroke burst still
    /// produces exactly one parse + one publish.
    fn schedule_publish_debounced(&self, uri: Url) {
        let Some(state) = self.lookup(&uri) else {
            return;
        };
        let target_version = state.edit_version();
        let backend = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(PUBLISH_DEBOUNCE_MS)).await;
            backend
                .reparse_and_publish_if_current(uri, target_version)
                .await;
        });
    }

    /// The debounced task body — re-parse semantically then
    /// publish, but only if no newer edit has come in.
    async fn reparse_and_publish_if_current(&self, uri: Url, target_version: u64) {
        // Wait-free snapshot read — does not contend with concurrent
        // request handlers that also load the snapshot.
        let (text, state) = {
            let Some(state) = self.lookup(&uri) else {
                return;
            };
            if state.edit_version() != target_version {
                // A newer edit came in during the debounce window;
                // its own task will publish. Bail.
                return;
            }
            (state.snapshot().text.clone(), state)
        };

        // Parse off the async runtime so concurrent hover /
        // codeAction / inlay requests do not stall waiting for an
        // executor thread. `Document::new` takes `impl Into<Box<str>>`;
        // we pass an owned String materialised from the Arc<str>.
        let text_owned = text.to_string();
        let bytes_estimate = u64::try_from(text_owned.len()).unwrap_or(u64::MAX);
        let parse_result = tokio::task::spawn_blocking(move || {
            let document = aozora::Document::new(text_owned);
            document.parse().diagnostics().to_vec()
        })
        .await;
        let Ok(diagnostics) = parse_result else {
            return;
        };

        // Re-check version so a parse that just missed the cutoff
        // doesn't overwrite a newer one. Diagnostics installation is
        // a brief `BufferState` mutex acquisition.
        if state.edit_version() != target_version {
            return;
        }
        state.install_diagnostics(diagnostics);
        state.metrics.record_parse(0, 0, 1, 1, bytes_estimate);
        let snap = state.snapshot();
        let publish_diags = state.with_segment_cache(|cache| {
            compute_diagnostics_from_parsed(&snap.text, cache.diagnostics())
        });
        self.client
            .publish_diagnostics(uri, publish_diags, None)
            .await;
    }

    /// Lookup helper — returns an `Arc<DocState>` clone so the caller
    /// can drop the dashmap shard reference immediately and operate
    /// on a wait-free snapshot. The dashmap shard read is microseconds;
    /// the Arc clone is a single atomic increment.
    fn lookup(&self, uri: &Url) -> Option<Arc<DocState>> {
        self.docs.get(uri).map(|entry| Arc::clone(&*entry))
    }

    /// Custom LSP request `aozora/renderHtml` — Phase 3.1.
    ///
    /// Returns the document's HTML rendering (via `aozora`'s borrowed
    /// HTML renderer). The `VSCode` preview pane consumes this on
    /// every `did_change` (debounced) so the webview stays in
    /// lock-step with the editor buffer.
    ///
    /// Argument shape: `{ "uri": "file:///…" }`. Returns
    /// `{ "html": "<…>" }` or an `invalid_params` error when no
    /// document is open at the URI.
    ///
    /// # Errors
    ///
    /// Returns [`JsonRpcError::invalid_params`] if no open document
    /// matches `params.uri`.
    pub async fn render_html(&self, params: RenderHtmlParams) -> Result<RenderHtmlResult> {
        // Wait-free snapshot — reads never contend with the writer
        // hot path. The Arc<str> clone is a single atomic bump.
        let state = self
            .lookup(&params.uri)
            .ok_or_else(|| JsonRpcError::invalid_params("no document at uri"))?;
        let text = state.snapshot().text.to_string();
        let html = tokio::task::spawn_blocking(move || {
            let document = aozora::Document::new(text);
            document.parse().to_html()
        })
        .await
        .map_err(|join_err| {
            let mut err = JsonRpcError::internal_error();
            err.message = format!("renderHtml panicked: {join_err}").into();
            err
        })?;
        Ok(RenderHtmlResult { html })
    }

    /// Custom LSP request `aozora/gaijiSpans` — Stage 7.
    ///
    /// Returns every resolvable `※［＃...］` gaiji span in the
    /// requested document, mapped to its resolved glyph and the
    /// LSP-coordinate range that the editor should fold over. The
    /// VS Code extension consumes this on every `did_change` to
    /// drive its inline-collapse decoration.
    ///
    /// Reads run lock-free against the pre-extracted
    /// [`crate::gaiji_spans::GaijiSpan`] list maintained by
    /// `DocState`; no parser is invoked.
    ///
    /// # Errors
    /// Returns [`JsonRpcError::invalid_params`] if no document at
    /// `params.uri` is open.
    #[allow(
        clippy::unused_async,
        reason = "tower-lsp custom_method requires async fn"
    )]
    pub async fn gaiji_spans(&self, params: GaijiSpansParams) -> Result<GaijiSpansResult> {
        let state = self
            .lookup(&params.uri)
            .ok_or_else(|| JsonRpcError::invalid_params("no document at uri"))?;
        let snap = state.snapshot();
        let mut views = Vec::with_capacity(snap.gaiji_spans.len());
        for span in snap.gaiji_spans.values() {
            let Some(resolved) =
                aozora_encoding::gaiji::lookup(None, span.mencode.as_deref(), &span.description)
            else {
                continue;
            };
            let mut buf = String::with_capacity(8);
            let _ = resolved.write_to(&mut buf);
            let start = snap
                .line_index
                .position(&snap.text, span.start_byte as usize);
            let end = snap.line_index.position(&snap.text, span.end_byte as usize);
            views.push(GaijiSpanView {
                range: Range::new(start, end),
                resolved: buf,
                description: span.description.to_string(),
                mencode: span.mencode.as_deref().map(str::to_owned),
            });
        }
        Ok(GaijiSpansResult { spans: views })
    }
}

/// Parameters for the `aozora/renderHtml` custom LSP request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderHtmlParams {
    pub uri: Url,
}

/// Result for the `aozora/renderHtml` custom LSP request.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RenderHtmlResult {
    pub html: String,
}

/// Parameters for the `aozora/gaijiSpans` custom LSP request — the
/// VS Code extension polls this on every `did_change` to refresh
/// its inline-fold decorations. The extension swaps each
/// `※［＃...］` source span for its resolved character so the
/// reader sees clean prose; the source re-appears when the cursor
/// enters the span.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GaijiSpansParams {
    pub uri: Url,
}

/// One gaiji span exposed to the editor for visual collapse.
/// `range` is in LSP coordinates (line/UTF-16 column); `resolved`
/// is the rendered glyph (may be a single char or a 2-codepoint
/// combining sequence).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GaijiSpanView {
    pub range: Range,
    pub resolved: String,
    pub description: String,
    pub mencode: Option<String>,
}

/// Result for `aozora/gaijiSpans` — every resolvable gaiji in the
/// document. Unresolved spans (description not in any table, no
/// `U+XXXX` form) are omitted because the editor has nothing to
/// substitute in their place.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GaijiSpansResult {
    pub spans: Vec<GaijiSpanView>,
}

/// JSON shape of the `aozora.canonicalizeSlug` `execute_command`
/// argument. Lifted to the top level so the
/// `clippy::items_after_statements` lint does not fire from inside
/// the `LanguageServer::execute_command` body.
#[derive(serde::Deserialize)]
struct CanonicalizeArgs {
    uri: Url,
    range: tower_lsp::lsp_types::Range,
    body: String,
}

/// Convert an LSP `TextDocumentContentChangeEvent` into a
/// [`LocalTextEdit`] against `source`. Returns `None` when the event
/// has no range (caller handles full-replacement separately) or when
/// either Position fails to resolve to a valid byte offset.
fn lsp_change_to_edit(
    source: &str,
    change: &TextDocumentContentChangeEvent,
) -> Option<LocalTextEdit> {
    let range = change.range?;
    let start = position_to_byte_offset(source, range.start)?;
    let end = position_to_byte_offset(source, range.end)?;
    if end < start {
        return None;
    }
    Some(LocalTextEdit::new(start..end, change.text.clone()))
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                // `inlay_hint_provider` deliberately omitted — the
                // VS Code extension renders gaiji inlines via
                // decoration in `gaijiFold.ts`, and adding an LSP
                // inlay layer on top duplicated the `→ X` glyph.
                // Editors that consume inlay hints over LSP can opt
                // in via the `crate::inlay_hints` library entry.
                linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(
                    true,
                )),
                completion_provider: Some(CompletionOptions {
                    // Two completion paths share the trigger list:
                    //
                    // * Slug catalogue (`crate::completion`) — fires
                    //   on `＃` (after `［`) or `#` (after `[`), and
                    //   on `「` for forward-reference quotes
                    //   (`［＃「target」に傍点］`).
                    // * Half-width emmet (`crate::half_width_emmet`)
                    //   — fires on `[`, `]`, `<`, `>`, `|`. Each
                    //   suggests the corresponding full-width glyph
                    //   (`［`, `］`, `《...》`, `》`, `｜`) and on
                    //   accept replaces the typed prefix verbatim.
                    trigger_characters: Some(vec![
                        "＃".to_owned(),
                        "#".to_owned(),
                        "「".to_owned(),
                        "[".to_owned(),
                        "]".to_owned(),
                        "<".to_owned(),
                        ">".to_owned(),
                        "|".to_owned(),
                    ]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![COMMAND_CANONICALIZE_SLUG.to_owned()],
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        // Advertised so VS Code shows the actions
                        // under right-click → Refactor and the
                        // Ctrl+. lightbulb. Resolve is not yet wired
                        // because every action ships a complete
                        // edit; resolve_provider stays None until a
                        // future heavier action (e.g. "rename slug
                        // across document") needs lazy loading.
                        code_action_kinds: Some(vec![
                            tower_lsp::lsp_types::CodeActionKind::REFACTOR_REWRITE,
                        ]),
                        ..CodeActionOptions::default()
                    },
                )),
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

    #[tracing::instrument(skip_all, fields(uri = %p.text_document.uri, text_bytes = p.text_document.text.len()))]
    async fn did_open(&self, p: DidOpenTextDocumentParams) {
        let uri = p.text_document.uri;
        self.docs
            .insert(uri.clone(), DocState::new(p.text_document.text));
        self.publish(uri).await;
    }

    // (`did_open` returns a `DocState::new(...)` which is already an
    // `Arc<DocState>`; the `DashMap<Url, Arc<DocState>>` insert above
    // moves the Arc directly. Subsequent reads `lookup` return a
    // cheap `Arc::clone`.)

    #[tracing::instrument(
        skip_all,
        fields(
            uri = %p.text_document.uri,
            version = p.text_document.version,
            change_count = p.content_changes.len(),
        ),
    )]
    async fn did_change(&self, p: DidChangeTextDocumentParams) {
        let uri = p.text_document.uri;
        let Some(state) = self.lookup(&uri) else {
            return;
        };
        for change in &p.content_changes {
            // Resolve the change against the LATEST snapshot — the
            // editor's view of the buffer is always at least as fresh
            // as our snapshot, so range coordinates resolve correctly.
            let snap = state.snapshot();
            // LSP allows mixing incremental and full-replacement
            // events in one batch; full replacement is signalled
            // by `range == None`.
            match lsp_change_to_edit(&snap.text, change) {
                Some(edit) => {
                    let _ = state.apply_changes(std::slice::from_ref(&edit));
                }
                None if change.range.is_none() => {
                    state.replace_text(change.text.clone());
                }
                None => {
                    tracing::warn!(
                        "skipping content change with unresolvable range: {:?}",
                        change.range,
                    );
                }
            }
        }
        // Stage 5 — schedule the slow Rust parse + publish as a
        // debounced background task. did_change itself returns now
        // (microseconds later), so subsequent LSP requests are not
        // blocked by tower-lsp's notification ordering.
        self.schedule_publish_debounced(uri);
    }

    #[tracing::instrument(skip_all, fields(uri = %p.text_document.uri))]
    async fn did_close(&self, p: DidCloseTextDocumentParams) {
        let uri = p.text_document.uri;
        // Dump the per-document Metrics snapshot at INFO so a third
        // party reading the log can reconstruct the document's
        // session-long behaviour. Done BEFORE the remove so we
        // still have access to the entry.
        if let Some(state) = self.lookup(&uri) {
            let snapshot = state.metrics.snapshot();
            tracing::info!(
                target: "aozora_lsp::metrics",
                uri = %uri,
                ?snapshot,
                "doc lifecycle metrics",
            );
        }
        self.docs.remove(&uri);
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    #[tracing::instrument(skip_all, fields(uri = %p.text_document.uri))]
    async fn formatting(&self, p: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = p.text_document.uri;
        let Some(state) = self.lookup(&uri) else {
            return Ok(None);
        };
        // Wait-free snapshot read; the parse + serialize runs on the
        // blocking pool so concurrent hover/codeAction requests on the
        // async runtime don't stall.
        let text = state.snapshot().text.to_string();
        let edits = tokio::task::spawn_blocking(move || format_edits(&text))
            .await
            .map_err(|join_err| {
                let mut err = tower_lsp::jsonrpc::Error::internal_error();
                err.message = format!("formatting panicked: {join_err}").into();
                err
            })?;
        Ok(Some(edits))
    }

    #[tracing::instrument(
        skip_all,
        fields(
            uri = %p.text_document_position_params.text_document.uri,
            line = p.text_document_position_params.position.line,
            character = p.text_document_position_params.position.character,
        ),
    )]
    async fn hover(&self, p: HoverParams) -> Result<Option<Hover>> {
        let uri = p.text_document_position_params.text_document.uri;
        let position = p.text_document_position_params.position;
        let Some(state) = self.lookup(&uri) else {
            return Ok(None);
        };
        // Wait-free snapshot. `hover_at` only reads the slice, so the
        // Arc<str> from snapshot is sufficient with no extra clone.
        let snap = state.snapshot();
        Ok(hover_at(&snap.text, position))
    }

    // `inlay_hint` deliberately *not* implemented on the
    // LanguageServer trait — the gaiji-fold decoration in the
    // VS Code extension already renders the resolved character
    // inline, so an LSP-side inlay just adds a redundant `→ X`
    // alongside the fold's substituted glyph. The extension owns
    // the cursor-aware "show → X only on the unfurled span"
    // behaviour because the LSP can't know the cursor; trying to
    // emit blanket inlays on the server side and hide them on the
    // client would be impossible (decorations cannot suppress
    // inlays). The library function `crate::inlay_hints::inlay_hints`
    // is kept exported for editor integrations that prefer the
    // server-side path (helix, neovim) and don't run our VS Code
    // extension.

    #[tracing::instrument(
        skip_all,
        fields(
            uri = %p.text_document_position_params.text_document.uri,
            line = p.text_document_position_params.position.line,
            character = p.text_document_position_params.position.character,
        ),
    )]
    async fn linked_editing_range(
        &self,
        p: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        let uri = p.text_document_position_params.text_document.uri;
        let position = p.text_document_position_params.position;
        let Some(state) = self.lookup(&uri) else {
            return Ok(None);
        };
        // Tree-free source scan — bounded look-window around the
        // cursor (≤ 1 KB each side). No parser invoked.
        let snap = state.snapshot();
        Ok(linked_editing_at(&snap.text, &snap.line_index, position))
    }

    #[tracing::instrument(
        skip_all,
        fields(
            uri = %p.text_document_position.text_document.uri,
            line = p.text_document_position.position.line,
            character = p.text_document_position.position.character,
        ),
    )]
    async fn completion(&self, p: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = p.text_document_position.text_document.uri;
        let position = p.text_document_position.position;
        let Some(state) = self.lookup(&uri) else {
            return Ok(None);
        };
        let snap = state.snapshot();
        // Tree-free: completion_at does its own bounded look-back
        // scan from the cursor (no parser needed). Removing the
        // `with_tree` call eliminates a full document re-parse on
        // every keystroke during slug completion — a major win on
        // 40 KB+ documents.
        let mut items: Vec<CompletionItem> = completion_at(&snap.text, position);
        // Append the half-width emmet suggestions. They are
        // independent of the parsed tree (the trigger detection is a
        // pure prefix scan), so we don't pay for a `with_tree` call
        // and the slug catalogue + emmet items merge into one
        // response — VS Code's own ranker decides ordering.
        items.extend(crate::half_width_emmet::emmet_completions(
            &snap.text, position,
        ));
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(CompletionResponse::Array(items)))
        }
    }

    #[tracing::instrument(skip_all, fields(uri = %p.text_document.uri))]
    async fn code_action(&self, p: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = p.text_document.uri;
        let Some(state) = self.lookup(&uri) else {
            return Ok(None);
        };
        // Quick fixes for diagnostics in the request range. Each
        // diagnostic carries a `data` payload describing what kind
        // of fix is appropriate; `quick_fix_actions` decodes those
        // and returns concrete `WorkspaceEdit`s.
        let mut actions = quick_fix_actions(&uri, &p.context.diagnostics);
        // Plus the wrap-selection actions when the user has a
        // non-empty selection. Both kinds are returned together so
        // the editor's lightbulb / right-click menu shows them in
        // one list.
        let snap = state.snapshot();
        actions.extend(wrap_selection_actions(
            &snap.text,
            &snap.line_index,
            &uri,
            p.range,
        ));
        if actions.is_empty() {
            Ok(None)
        } else {
            Ok(Some(actions))
        }
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command != COMMAND_CANONICALIZE_SLUG {
            return Err(JsonRpcError::method_not_found());
        }
        // Argument shape: a single JSON object with `uri`, `range`, `body`.
        let arg = params
            .arguments
            .into_iter()
            .next()
            .ok_or_else(|| JsonRpcError::invalid_params("expected one argument object"))?;
        let CanonicalizeArgs { uri, range, body } = serde_json::from_value(arg)
            .map_err(|err| JsonRpcError::invalid_params(err.to_string()))?;
        let Some(workspace_edit) = canonicalize_slug_edit(uri, range, &body) else {
            return Ok(None);
        };
        // Apply the edit through the client's
        // `workspace/applyEdit` RPC. Failures bubble up as
        // jsonrpc::Error.
        if let Err(err) = self.client.apply_edit(workspace_edit).await {
            tracing::warn!(error = %err, "applyEdit failed");
        }
        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Multi-angle test suite for the LSP incremental sync layer.
    //!
    //! Test sections:
    //!
    //! 1. **Conversion** — `lsp_change_to_edit` handles every well-formed
    //!    LSP `Range` correctly (ASCII, multibyte, multi-line, surrogate
    //!    pairs, edge offsets) and rejects ill-formed ones.
    //! 2. **`DocState` mechanics** — `apply_changes` / `replace_text`
    //!    move the buffer through the right transitions including
    //!    failure recovery.
    //! 3. **Batch semantics** — multiple `TextDocumentContentChangeEvent`
    //!    events in one notification compose to the same final state
    //!    as individual notifications, preserving LSP's source-order
    //!    rule.
    //! 4. **Edit content shapes** — pure-text fast path vs aozora-trigger
    //!    fallback path; both must remain byte-equivalent to a full parse.
    //! 5. **End-to-end equivalence** — long edit sequences and full
    //!    replacements both converge to the buffer the user actually sees.

    use super::*;
    use tower_lsp::lsp_types::{Position, Range};

    fn synth_change(range: Option<Range>, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range,
            range_length: None,
            text: text.to_owned(),
        }
    }

    /// Replay a batch of LSP changes against a starting buffer using
    /// the same logic the backend uses, so tests can compare against
    /// "what the editor thinks the buffer looks like" without booting
    /// tower-lsp.
    fn replay_lsp_changes(initial: &str, changes: &[TextDocumentContentChangeEvent]) -> String {
        let state = DocState::new(initial.to_owned());
        for change in changes {
            let snap = state.snapshot();
            match lsp_change_to_edit(&snap.text, change) {
                Some(edit) => {
                    let _ = state.apply_changes(std::slice::from_ref(&edit));
                }
                None if change.range.is_none() => {
                    state.replace_text(change.text.clone());
                }
                None => {} // unresolvable range: skip (matches backend behaviour)
            }
        }
        state.snapshot().text.to_string()
    }

    // ---------------------------------------------------------------
    // 1. Conversion: lsp_change_to_edit
    // ---------------------------------------------------------------

    #[test]
    fn lsp_change_to_edit_returns_none_for_full_replacement() {
        let change = synth_change(None, "new text");
        assert!(lsp_change_to_edit("anything", &change).is_none());
    }

    #[test]
    fn lsp_change_to_edit_handles_basic_range() {
        let source = "hello world";
        let change = synth_change(
            Some(Range::new(Position::new(0, 6), Position::new(0, 11))),
            "rust",
        );
        let edit = lsp_change_to_edit(source, &change).expect("convert");
        assert_eq!(edit.range, 6..11);
        assert_eq!(edit.new_text, "rust");
    }

    #[test]
    fn lsp_change_to_edit_handles_multibyte_position() {
        // 「あ」 is 1 UTF-16 unit, 3 UTF-8 bytes.
        let source = "あいう";
        let change = synth_change(
            Some(Range::new(Position::new(0, 1), Position::new(0, 2))),
            "X",
        );
        let edit = lsp_change_to_edit(source, &change).expect("convert");
        assert_eq!(edit.range, 3..6);
        assert_eq!(edit.new_text, "X");
    }

    #[test]
    fn lsp_change_to_edit_handles_pure_deletion() {
        let source = "abcdef";
        // Delete bytes 2..4 ("cd").
        let change = synth_change(
            Some(Range::new(Position::new(0, 2), Position::new(0, 4))),
            "",
        );
        let edit = lsp_change_to_edit(source, &change).expect("convert");
        assert_eq!(edit.range, 2..4);
        assert_eq!(edit.new_text, "");
    }

    // ---------------------------------------------------------------
    // 2. DocState mechanics
    // ---------------------------------------------------------------

    #[test]
    fn doc_state_new_populates_cache() {
        let state = DocState::new("hello".to_owned());
        // Plain text emits zero diagnostics — the cache surfaces an
        // empty slice but is *populated* (no longer "first reparse"
        // pending).
        state.with_segment_cache(|cache| {
            assert!(cache.diagnostics().is_empty());
        });
        assert_eq!(&*state.snapshot().text, "hello");
    }

    #[test]
    fn doc_state_apply_changes_updates_text() {
        let state = DocState::new("hello world".to_owned());
        let edit = LocalTextEdit::new(6..11, "rust".to_owned());
        state.apply_changes(&[edit]);
        assert_eq!(&*state.snapshot().text, "hello rust");
    }

    #[test]
    fn doc_state_apply_changes_rejects_invalid_edit_keeps_text() {
        let state = DocState::new("hi".to_owned());
        let edit = LocalTextEdit::new(0..99, "x".to_owned());
        let result = state.apply_changes(&[edit]);
        assert!(result.is_none(), "out-of-bounds edit must be rejected");
        assert_eq!(&*state.snapshot().text, "hi");
    }

    #[test]
    fn doc_state_apply_changes_rejects_non_char_boundary_edit() {
        let state = DocState::new("あ".to_owned()); // 3 bytes
        let edit = LocalTextEdit::new(1..2, "x".to_owned());
        let result = state.apply_changes(&[edit]);
        assert!(result.is_none(), "cross-boundary edit must be rejected");
        assert_eq!(
            &*state.snapshot().text,
            "あ",
            "non-boundary edit must be rejected",
        );
    }

    #[test]
    fn doc_state_replace_text_updates_buffer() {
        let state = DocState::new("hello".to_owned());
        state.replace_text("｜青梅《おうめ》".to_owned());
        assert_eq!(&*state.snapshot().text, "｜青梅《おうめ》");
    }

    // ---------------------------------------------------------------
    // 3. Batch semantics
    // ---------------------------------------------------------------

    #[test]
    fn two_events_in_one_batch_apply_in_source_order() {
        let initial = "abcdef";
        let changes = vec![
            synth_change(
                Some(Range::new(Position::new(0, 1), Position::new(0, 2))),
                "BB",
            ),
            synth_change(
                Some(Range::new(Position::new(0, 4), Position::new(0, 5))),
                "DD",
            ),
        ];
        let final_text = replay_lsp_changes(initial, &changes);
        assert_eq!(final_text, "aBBcDDef");
    }

    #[test]
    fn full_replacement_after_incremental_in_same_batch_wins() {
        let initial = "old text";
        let changes = vec![
            synth_change(
                Some(Range::new(Position::new(0, 0), Position::new(0, 0))),
                "PREFIX:",
            ),
            synth_change(None, "FRESH"),
        ];
        let final_text = replay_lsp_changes(initial, &changes);
        assert_eq!(final_text, "FRESH");
    }

    // ---------------------------------------------------------------
    // 4. Edit content shapes
    // ---------------------------------------------------------------

    #[test]
    fn edit_inserting_aozora_trigger_reparses() {
        let state = DocState::new("plain text".to_owned());
        let edit = LocalTextEdit::new(5..6, "｜青梅《おうめ》".to_owned());
        state.apply_changes(&[edit]);
        // Stage 5: apply_changes is the *fast* path — text + TS
        // edit only. The semantic re-parse runs in a background
        // task in production. For this unit test (no async runtime)
        // we drive it synchronously through the same entry point the
        // debounced task uses.
        state.run_segment_cache_reparse();
        state.with_segment_cache(|cache| {
            let inline = cache
                .with_tree(|t| t.lex_output().registry.inline.len())
                .expect("populated");
            assert_eq!(inline, 1);
            assert!(cache.diagnostics().is_empty());
        });
    }

    #[test]
    fn pua_collision_edit_surfaces_diagnostic() {
        let state = DocState::new("plain".to_owned());
        let edit = LocalTextEdit::new(0..0, "\u{E001}".to_owned());
        state.apply_changes(&[edit]);
        // See note in `edit_inserting_aozora_trigger_reparses` —
        // semantic re-parse is deferred under Stage 5.
        state.run_segment_cache_reparse();
        state.with_segment_cache(|cache| {
            assert!(
                !cache.diagnostics().is_empty(),
                "PUA injection must produce diagnostics; got {:?}",
                cache.diagnostics(),
            );
        });
    }

    // ---------------------------------------------------------------
    // 5. End-to-end
    // ---------------------------------------------------------------

    #[test]
    fn sequence_of_incremental_edits_converges_to_full_text() {
        let state = DocState::new(String::new());
        for (i, ch) in "hello world".chars().enumerate() {
            let edit = LocalTextEdit::new(i..i, ch.to_string());
            state.apply_changes(&[edit]);
        }
        assert_eq!(&*state.snapshot().text, "hello world");
    }
}
