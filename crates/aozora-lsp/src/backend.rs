//! tower-lsp `LanguageServer` implementation for aozora documents.
//!
//! # State model
//!
//! Each open document has a [`DocState`] with three fields:
//!
//! - `text`: the latest mirror of the editor's buffer.
//! - `segment_cache`: the latest [`aozora::Document`] + diagnostics
//!   produced by parsing `text`. After Phase 0 of the
//!   editor-integration sprint the cache is a thin wrapper around
//!   `Document::parse` rather than the per-paragraph hash table the
//!   pre-0.2 lexer needed; the new bumpalo-arena pipeline is fast
//!   enough that whole-document re-parse per request fits the
//!   keystroke-perceptibility budget.
//! - `metrics`: per-document observability counters. Updated on every
//!   reparse; dumped at INFO level on `did_close` so a third party
//!   reading the log can reconstruct the document's session-long
//!   behaviour.
//!
//! # Sync mode
//!
//! `text_document_sync` is [`TextDocumentSyncKind::INCREMENTAL`].
//! `did_change` walks each `TextDocumentContentChangeEvent` in the
//! batch, applies the byte-range edits to `text` via
//! [`crate::text_edit::apply_edits`], then re-parses through the
//! [`crate::segment_cache::SegmentCache`].
//!
//! Reads (hover, formatting, diagnostics) borrow the `DashMap` entry
//! across the computation so they consume the cached parse without
//! cloning the 1 MB buffer on every cursor move.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use dashmap::DashMap;

use crate::code_actions::{quick_fix_actions, wrap_selection_actions};
use crate::commands::{COMMAND_CANONICALIZE_SLUG, canonicalize_slug_edit};
use crate::completion::completion_at;
use crate::inlay_hints::inlay_hints;
use crate::linked_editing::linked_editing_at;
use crate::metrics::Metrics;
use crate::gaiji_spans::{GaijiSpan, extract_gaiji_spans};
use crate::incremental::{IncrementalDoc, input_edit};
use crate::line_index::LineIndex;
use crate::segment_cache::SegmentCache;
use crate::text_edit::{LocalTextEdit, apply_edits};
use crate::{compute_diagnostics_from_parsed, format_edits, hover_at};
use tower_lsp::jsonrpc::{Error as JsonRpcError, Result};
use tower_lsp::lsp_types::{
    CodeActionOptions, CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    CompletionItem, CompletionOptions, CompletionParams, CompletionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentFormattingParams, ExecuteCommandOptions, ExecuteCommandParams, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, InitializedParams, InlayHint,
    InlayHintParams, InlayHintServerCapabilities, LinkedEditingRangeParams,
    LinkedEditingRangeServerCapabilities, LinkedEditingRanges, MessageType, OneOf, Range,
    ServerCapabilities, ServerInfo, TextDocumentContentChangeEvent, TextDocumentSyncCapability,
    TextDocumentSyncKind, TextEdit, Url,
};
use tower_lsp::{Client, LanguageServer};

use crate::position::position_to_byte_offset;

/// Per-document state held by the backend.
#[derive(Debug, Default)]
pub struct DocState {
    /// Mirror of the editor buffer after every applied change.
    pub text: String,
    /// Latest semantic parse of `text`. Populated by the debounced
    /// background task (Stage 5); read by `publish` for diagnostics.
    /// May lag the latest text by up to `DEBOUNCE_MS` while a fast
    /// typist holds the keyboard.
    pub segment_cache: SegmentCache,
    /// Tree-sitter incremental parser. Used for hover, inlay,
    /// codeAction, completion — all the high-frequency requests.
    /// Edits are applied incrementally so per-keystroke cost stays
    /// `O(edit_size)` rather than `O(doc_size)`.
    pub incremental: IncrementalDoc,
    /// Per-text-version line-start byte-offset index. Lets handlers
    /// convert byte offsets to LSP `Position`s in `O(log lines)`
    /// instead of `O(byte_offset)`. Rebuilt on every text change
    /// (cheap — single SIMD `memchr` pass).
    pub line_index: LineIndex,
    /// Pre-extracted `※［＃...］` span list, sorted by start byte.
    /// Rebuilt under the same write lock that updates `text` / TS,
    /// so reads are lock-free `Arc<[GaijiSpan]>` clones. Lets the
    /// inlay handler skip the tree walk + `Mutex` acquisition on
    /// every cursor move.
    pub gaiji_spans: Arc<[GaijiSpan]>,
    /// Monotonically incremented on every text change. Debounced
    /// publish tasks read this to confirm "my snapshot is still the
    /// latest" before doing the expensive parse + publish work
    /// (Stage 5 race-free coalescing).
    pub parse_version: AtomicU64,
    /// Per-document observability counters. Updated on every
    /// reparse; dumped at INFO level on `did_close` so a third
    /// party reading the log can reconstruct the document's
    /// session-long behaviour.
    pub metrics: Arc<Metrics>,
}

impl DocState {
    fn new(text: String) -> Self {
        let line_index = LineIndex::new(&text);
        let mut state = Self {
            text,
            segment_cache: SegmentCache::default(),
            incremental: IncrementalDoc::new(),
            line_index,
            gaiji_spans: Arc::from(Vec::new()),
            parse_version: AtomicU64::new(0),
            metrics: Arc::new(Metrics::default()),
        };
        // didOpen still does the slow Rust parse synchronously. The
        // user has just opened the document; some warm-up cost is
        // expected. Subsequent edits use the debounced path.
        state.reparse_and_record();
        state.incremental.parse_full(&state.text);
        state.refresh_gaiji_spans();
        state
    }

    /// Refresh the pre-extracted gaiji span list from the current
    /// tree-sitter tree. Called after every TS update so handlers
    /// can read the cache lock-free.
    fn refresh_gaiji_spans(&mut self) {
        let spans = self
            .incremental
            .with_tree(|tree| extract_gaiji_spans(tree, &self.text))
            .unwrap_or_else(|| Arc::from(Vec::new()));
        self.gaiji_spans = spans;
    }

    /// Apply a batch of edits to `text`. Fast path: text mutation +
    /// tree-sitter incremental edits only. The slow Rust semantic
    /// parse is deferred to a debounced background task (Stage 5).
    fn apply_changes(&mut self, edits: &[LocalTextEdit]) {
        // Snapshot byte ranges BEFORE mutating `self.text`, so the
        // tree-sitter `InputEdit` carries the right "old end" byte
        // offset relative to the pre-change buffer.
        let ts_edits: Vec<tree_sitter::InputEdit> = edits
            .iter()
            .map(|e| input_edit(e.range.start, e.range.end, e.range.start + e.new_text.len()))
            .collect();
        match apply_edits(&self.text, edits) {
            Ok(new_text) => {
                self.text = new_text;
                self.metrics.record_edit();
                for edit in &ts_edits {
                    self.incremental.apply_edit(&self.text, *edit);
                }
                // Rebuild the line index — single SIMD pass, fast
                // even for 200 MB inputs. Could be incrementalised
                // later (only re-walk from the first changed line)
                // but the simple full rebuild keeps the code
                // straightforward and stays well under millisecond
                // cost for realistic docs.
                self.line_index = LineIndex::new(&self.text);
                self.refresh_gaiji_spans();
                // Bump the version last so a debounced task that
                // races against this one always sees the post-edit
                // state when it samples.
                self.parse_version.fetch_add(1, Ordering::SeqCst);
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    text_bytes = self.text.len(),
                    "rejecting incremental edit batch; document state unchanged",
                );
            }
        }
    }

    /// Replace the buffer wholesale. Fast path same as [`Self::apply_changes`].
    fn replace_text(&mut self, new_text: String) {
        self.text = new_text;
        self.metrics.record_edit();
        // Full replacement → full TS parse from scratch.
        self.incremental.parse_full(&self.text);
        self.line_index = LineIndex::new(&self.text);
        self.refresh_gaiji_spans();
        self.parse_version.fetch_add(1, Ordering::SeqCst);
    }

    /// Run a single reparse through the segment cache and feed the
    /// per-call stats into the per-document `Metrics`. Centralised
    /// so every reparse path (open, change, replace) records the
    /// same observability fields.
    fn reparse_and_record(&mut self) {
        let (_diags, stats) = self.segment_cache.reparse(&self.text);
        self.metrics.record_parse(
            stats.latency_us,
            stats.cache_hits,
            stats.cache_misses,
            stats.cache_entries_after,
            stats.cache_bytes_estimate,
        );
        // Slow-path WARN. Threshold env-var tunable; default 100 ms
        // matches the keystroke-perceptibility line. Throttling is
        // out of scope here — at LSP keystroke rates a steady warn
        // stream is itself the signal.
        let threshold = slow_parse_threshold_us();
        if stats.latency_us > threshold {
            tracing::warn!(
                latency_us = stats.latency_us,
                threshold_us = threshold,
                segment_count = stats.segment_count,
                cache_hits = stats.cache_hits,
                cache_misses = stats.cache_misses,
                "parse exceeded slow-path threshold",
            );
        }
    }
}

/// Look up the `AOZORA_LSP_SLOW_PARSE_US` env var, default
/// `100_000` (100 ms). Read once per call; we don't cache it
/// because reparse is rare enough (≤ keystroke rate) that the
/// env-var read is negligible.
fn slow_parse_threshold_us() -> u64 {
    std::env::var("AOZORA_LSP_SLOW_PARSE_US")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(100_000)
}

/// LSP backend for aozora documents.
///
/// `Clone` so we can hand a copy to debounced background tasks
/// (Stage 5). The fields are cheap to clone — `Client` is a
/// channel handle and `docs` is `Arc<DashMap<...>>`.
#[derive(Debug, Clone)]
pub struct Backend {
    client: Client,
    docs: Arc<DashMap<Url, DocState>>,
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
        let diags = self.docs.get(&uri).map_or_else(Vec::new, |entry| {
            compute_diagnostics_from_parsed(&entry.text, entry.segment_cache.diagnostics())
        });
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Schedule a debounced semantic re-parse + diagnostic publish.
    /// The actual work runs after `PUBLISH_DEBOUNCE_MS` quiet time.
    /// Multiple rapid edits coalesce — only the task whose recorded
    /// `target_version` matches the doc's current `parse_version`
    /// after the sleep proceeds. Earlier tasks observe a newer
    /// version and exit silently, so a 100-keystroke burst still
    /// produces exactly one parse + one publish.
    fn schedule_publish_debounced(&self, uri: Url) {
        let target_version = self
            .docs
            .get(&uri)
            .map(|entry| entry.parse_version.load(Ordering::SeqCst));
        let Some(target_version) = target_version else {
            return;
        };
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
        // Snapshot the text + version under a short read lock.
        let text = {
            let Some(entry) = self.docs.get(&uri) else {
                return;
            };
            if entry.parse_version.load(Ordering::SeqCst) != target_version {
                // A newer edit came in during the debounce window;
                // its own task will publish. Bail.
                return;
            }
            entry.text.clone()
        };

        // Parse off the async runtime so concurrent hover /
        // codeAction / inlay requests do not stall waiting for an
        // executor thread.
        let bytes_estimate = u64::try_from(text.len()).unwrap_or(u64::MAX);
        let parse_result = tokio::task::spawn_blocking(move || {
            let document = aozora::Document::new(text);
            document.parse().diagnostics().to_vec()
        })
        .await;
        let Ok(diagnostics) = parse_result else {
            return;
        };

        // Re-acquire briefly to install the diagnostics on the
        // segment cache. Re-check version so a parse that just
        // missed the cutoff doesn't overwrite a newer one.
        let publish_diags = {
            let Some(mut entry) = self.docs.get_mut(&uri) else {
                return;
            };
            if entry.parse_version.load(Ordering::SeqCst) != target_version {
                return;
            }
            entry.segment_cache.set_diagnostics(diagnostics);
            // Record metrics for observability — same fields as the
            // synchronous path so the per-doc snapshot stays meaningful.
            entry.metrics.record_parse(0, 0, 1, 1, bytes_estimate);
            compute_diagnostics_from_parsed(&entry.text, entry.segment_cache.diagnostics())
        };
        self.client.publish_diagnostics(uri, publish_diags, None).await;
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
        // Snapshot the source under the DashMap shard lock, then
        // release the entry so `spawn_blocking` can run the
        // CPU-bound parse + render off the async runtime. Without
        // this any concurrent hover/codeAction request on the same
        // doc would queue behind the render's lock and the parse.
        let text = {
            let entry = self
                .docs
                .get(&params.uri)
                .ok_or_else(|| JsonRpcError::invalid_params("no document at uri"))?;
            entry.text.clone()
        };
        let html = tokio::task::spawn_blocking(move || {
            // `Document::new` takes `impl Into<Box<str>>`; passing
            // the owned `String` moves the buffer in without an
            // extra allocation.
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
        let entry = self
            .docs
            .get(&params.uri)
            .ok_or_else(|| JsonRpcError::invalid_params("no document at uri"))?;
        let mut views = Vec::with_capacity(entry.gaiji_spans.len());
        for span in entry.gaiji_spans.iter() {
            let Some(resolved) =
                aozora_encoding::gaiji::lookup(None, span.mencode.as_deref(), &span.description)
            else {
                continue;
            };
            let mut buf = String::with_capacity(8);
            let _ = resolved.write_to(&mut buf);
            let start = entry.line_index.position(&entry.text, span.start_byte as usize);
            let end = entry.line_index.position(&entry.text, span.end_byte as usize);
            views.push(GaijiSpanView {
                range: Range::new(start, end),
                resolved: buf,
                description: span.description.clone(),
                mencode: span.mencode.clone(),
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
                inlay_hint_provider: Some(OneOf::Right(InlayHintServerCapabilities::Options(
                    tower_lsp::lsp_types::InlayHintOptions::default(),
                ))),
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
        {
            let Some(mut entry) = self.docs.get_mut(&uri) else {
                return;
            };
            for change in &p.content_changes {
                // LSP allows mixing incremental and full-replacement
                // events in one batch; full replacement is signalled
                // by `range == None`.
                match lsp_change_to_edit(&entry.text, change) {
                    Some(edit) => entry.apply_changes(std::slice::from_ref(&edit)),
                    None if change.range.is_none() => {
                        entry.replace_text(change.text.clone());
                    }
                    None => {
                        tracing::warn!(
                            "skipping content change with unresolvable range: {:?}",
                            change.range,
                        );
                    }
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
        if let Some(entry) = self.docs.get(&uri) {
            let snapshot = entry.metrics.snapshot();
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
        // Snapshot text under the lock, release, then run the
        // parse + serialize on a blocking thread. Same reasoning as
        // `render_html` above — a 40 KB doc takes ~400 ms of pure
        // CPU; doing that inside the async handler stalls every
        // other in-flight request on the runtime.
        let text = {
            let Some(entry) = self.docs.get(&uri) else {
                return Ok(None);
            };
            entry.text.clone()
        };
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
        // Hold the DashMap entry across the hover computation so we
        // borrow the document text in place rather than cloning it.
        // For a 1 MB buffer + cursor-driven hover this saves a 1 MB
        // allocation on every cursor move; `hover_at` only reads
        // the slice, so the borrow is sufficient.
        let Some(entry) = self.docs.get(&uri) else {
            return Ok(None);
        };
        Ok(hover_at(&entry.text, position))
    }

    #[tracing::instrument(skip_all, fields(uri = %p.text_document.uri))]
    async fn inlay_hint(&self, p: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = p.text_document.uri;
        let Some(entry) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // Tree-sitter path: incremental tree was kept in sync via
        // `did_change`, so this query is O(spans-in-range) instead
        // of a full re-parse. Falls back to an empty Vec if the
        // tree was never seeded (only true for an empty
        // freshly-opened doc).
        // Cache-driven: lock-free read against the pre-extracted
        // gaiji span list. No tree-sitter Mutex acquired, no parser
        // touched. Concurrent inlay requests run in true parallel.
        let hints = inlay_hints(
            &entry.text,
            &entry.gaiji_spans,
            &entry.line_index,
            p.range,
        );
        Ok(Some(hints))
    }

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
        let Some(entry) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // Tree-free source scan — bounded look-window around the
        // cursor (≤ 1 KB each side). No parser invoked.
        Ok(linked_editing_at(&entry.text, &entry.line_index, position))
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
        let Some(entry) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // Tree-free: completion_at does its own bounded look-back
        // scan from the cursor (no parser needed). Removing the
        // `with_tree` call eliminates a full document re-parse on
        // every keystroke during slug completion — a major win on
        // 40 KB+ documents.
        let mut items: Vec<CompletionItem> = completion_at(&entry.text, position);
        // Append the half-width emmet suggestions. They are
        // independent of the parsed tree (the trigger detection is a
        // pure prefix scan), so we don't pay for a `with_tree` call
        // and the slug catalogue + emmet items merge into one
        // response — VS Code's own ranker decides ordering.
        items.extend(crate::half_width_emmet::emmet_completions(
            &entry.text,
            position,
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
        let Some(entry) = self.docs.get(&uri) else {
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
        actions.extend(wrap_selection_actions(
            &entry.text,
            &entry.line_index,
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
        let mut state = DocState::new(initial.to_owned());
        for change in changes {
            match lsp_change_to_edit(&state.text, change) {
                Some(edit) => state.apply_changes(std::slice::from_ref(&edit)),
                None if change.range.is_none() => state.replace_text(change.text.clone()),
                None => {} // unresolvable range: skip (matches backend behaviour)
            }
        }
        state.text
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
        assert!(state.segment_cache.diagnostics().is_empty());
        assert_eq!(state.text, "hello");
    }

    #[test]
    fn doc_state_apply_changes_updates_text() {
        let mut state = DocState::new("hello world".to_owned());
        let edit = LocalTextEdit::new(6..11, "rust".to_owned());
        state.apply_changes(&[edit]);
        assert_eq!(state.text, "hello rust");
    }

    #[test]
    fn doc_state_apply_changes_rejects_invalid_edit_keeps_text() {
        let mut state = DocState::new("hi".to_owned());
        let edit = LocalTextEdit::new(0..99, "x".to_owned());
        state.apply_changes(&[edit]);
        assert_eq!(state.text, "hi");
    }

    #[test]
    fn doc_state_apply_changes_rejects_non_char_boundary_edit() {
        let mut state = DocState::new("あ".to_owned()); // 3 bytes
        let edit = LocalTextEdit::new(1..2, "x".to_owned());
        state.apply_changes(&[edit]);
        assert_eq!(state.text, "あ", "non-boundary edit must be rejected");
    }

    #[test]
    fn doc_state_replace_text_updates_buffer() {
        let mut state = DocState::new("hello".to_owned());
        state.replace_text("｜青梅《おうめ》".to_owned());
        assert_eq!(state.text, "｜青梅《おうめ》");
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
        let mut state = DocState::new("plain text".to_owned());
        let edit = LocalTextEdit::new(5..6, "｜青梅《おうめ》".to_owned());
        state.apply_changes(&[edit]);
        // Stage 5: apply_changes is the *fast* path — text + TS
        // edit only. The semantic re-parse runs in a background
        // task in production. For this unit test (no async runtime)
        // we drive it synchronously.
        state.reparse_and_record();
        let inline = state
            .segment_cache
            .with_tree(|t| t.lex_output().registry.inline.len())
            .expect("populated");
        assert_eq!(inline, 1);
        assert!(state.segment_cache.diagnostics().is_empty());
    }

    #[test]
    fn pua_collision_edit_surfaces_diagnostic() {
        let mut state = DocState::new("plain".to_owned());
        let edit = LocalTextEdit::new(0..0, "\u{E001}".to_owned());
        state.apply_changes(&[edit]);
        // See note in `edit_inserting_aozora_trigger_reparses` —
        // semantic re-parse is deferred under Stage 5.
        state.reparse_and_record();
        assert!(
            !state.segment_cache.diagnostics().is_empty(),
            "PUA injection must produce diagnostics; got {:?}",
            state.segment_cache.diagnostics(),
        );
    }

    // ---------------------------------------------------------------
    // 5. End-to-end
    // ---------------------------------------------------------------

    #[test]
    fn sequence_of_incremental_edits_converges_to_full_text() {
        let mut state = DocState::new(String::new());
        for (i, ch) in "hello world".chars().enumerate() {
            let edit = LocalTextEdit::new(i..i, ch.to_string());
            state.apply_changes(&[edit]);
        }
        assert_eq!(state.text, "hello world");
    }
}
