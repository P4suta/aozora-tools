//! Per-document state — paragraph-first model.
//!
//! ## Why paragraph-first
//!
//! Bench data drove the architecture: tree-sitter's parse on the
//! aozora grammar is `O(doc-size)` (~33 ns/byte) regardless of edit
//! position, so any whole-document parse path tops out at ~220 ms
//! per keystroke on a 6 MB document. The fix isn't a smarter
//! incremental algorithm — it's segmenting the document into
//! `\n\n`-bounded paragraphs and only re-parsing the paragraph
//! containing each edit.
//!
//! Once the parser is paragraph-local, the **rest** of the per-edit
//! cost (rope materialisation, line-index build, gaiji-span walk)
//! also wants to be paragraph-local — otherwise each edit still
//! pays a doc-size memcpy + `LineIndex` SIMD scan even though only
//! one paragraph changed. So this module makes paragraph-shape
//! pervasive:
//!
//! - [`BufferState`] (writers): `Vec<MutParagraph>` + a single
//!   `Parser`. Each `MutParagraph` owns its `Rope` text and
//!   tree-sitter `Tree`.
//! - [`Snapshot`] (readers): `Arc<[Arc<ParagraphSnapshot>]>`. Each
//!   `ParagraphSnapshot` carries doc-absolute byte ranges + per-
//!   paragraph text / line-index / gaiji spans.
//! - **Doc-level views** ([`Snapshot::doc_text`],
//!   [`Snapshot::doc_line_index`], [`Snapshot::doc_gaiji_spans`])
//!   are lazily materialised once per snapshot via `OnceLock`.
//!   Handlers that still want a flat `&str` view pay one
//!   materialisation per snapshot generation; handlers that can
//!   iterate paragraphs directly skip it entirely.
//!
//! ## Edit flow
//!
//! `apply_changes(edits)` walks each edit, resolves it to a
//! paragraph index via binary search on cumulative byte offsets,
//! mutates that paragraph's rope, calls `MutParagraph::apply_edit`
//! to reparse just that paragraph. Cross-paragraph edits trigger a
//! merge-and-reparse of the affected paragraph range; oversized
//! paragraphs (> [`MAX_PARAGRAPH_BYTES`]) trigger a re-segment of
//! that paragraph's content.
//!
//! ## Reader / writer decoupling
//!
//! The `BufferState` mutex protects writers; readers go through
//! `Snapshot` via `ArcSwap`. ADR-0005 explains the wait-free read
//! property in detail. Per-paragraph snapshots add another layer:
//! unchanged paragraphs across snapshot generations are
//! `Arc::clone`d (single atomic increment), so a snapshot rebuild
//! after a small edit costs `O(1 paragraph rebuilt + N - 1
//! Arc-bumps)` rather than `O(doc)`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use ropey::Rope;
use tree_sitter::Parser;

use crate::gaiji_spans::GaijiSpan;
use crate::incremental::input_edit;
use crate::line_index::LineIndex;
use crate::metrics::Metrics;
use crate::paragraph::{
    MAX_PARAGRAPH_BYTES, MutParagraph, ParagraphSnapshot, build_paragraph_snapshot,
    paragraph_byte_ranges,
};
use crate::segment_cache::SegmentCache;
use crate::text_edit::{EditError, LocalTextEdit};

/// Slice `source` at `range`, build a new owned `Rope` from that
/// slice, and reparse it via `parser`. Used by every code path that
/// constructs a `MutParagraph` from a substring of a larger Rope —
/// `BufferState::new`, `replace`, `apply_across_paragraphs`,
/// `maybe_resegment_around`. Centralised so the
/// `byte_slice → Rope::from → reparse` sequence lives in exactly one
/// place.
fn paragraph_from_rope_slice(
    source: &Rope,
    range: std::ops::Range<usize>,
    parser: &mut Parser,
) -> MutParagraph {
    let slice = source.byte_slice(range);
    let mut paragraph = MutParagraph::new(Rope::from(slice));
    paragraph.reparse(parser);
    paragraph
}

// =====================================================================
// Mutable side: BufferState
// =====================================================================

/// Mutable per-document state. Held behind `DocState::buffer`.
///
/// `paragraphs` is the only source-of-truth field. Doc-absolute
/// byte offsets and the total byte length are derived on demand by
/// walking the paragraphs (`O(N)` where N is paragraph count, not
/// document size). At LSP keystroke rates with paragraph counts in
/// the low hundreds this is comfortably under a microsecond per
/// `apply_one_edit` call. The reader-side
/// [`Snapshot::paragraph_starts`] keeps the cumulative-offset table
/// for handlers that need binary-search-by-byte; we don't carry a
/// separate copy here so the writer side stays slim and there's a
/// single place where `paragraph_starts` is recomputed (snapshot
/// build).
///
/// The tree-sitter `Parser` lives here (one per doc) — paragraphs
/// share it serially. Parsers are cheap to keep around but `!Sync`,
/// so we don't spin up one per paragraph.
pub struct BufferState {
    pub paragraphs: Vec<MutParagraph>,
    pub parser: Parser,
    pub segment_cache: SegmentCache,
}

impl std::fmt::Debug for BufferState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BufferState")
            .field("paragraphs", &self.paragraphs.len())
            .finish_non_exhaustive()
    }
}

impl BufferState {
    fn new(text: String) -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .expect("tree-sitter-aozora language is compiled in");
        let rope = Rope::from(text);
        let ranges = paragraph_byte_ranges(&rope);
        let mut paragraphs: Vec<MutParagraph> = ranges
            .into_iter()
            .map(|range| paragraph_from_rope_slice(&rope, range, &mut parser))
            .collect();
        if paragraphs.is_empty() {
            // Empty document — keep one empty paragraph so the
            // rest of the code can assume non-empty `paragraphs`.
            paragraphs.push(MutParagraph::new(Rope::new()));
        }
        Self {
            paragraphs,
            parser,
            segment_cache: SegmentCache::default(),
        }
    }

    /// Total byte length of the document — sum of paragraph sizes.
    /// `O(N)` in paragraph count; called at most once per
    /// `validate_edits` invocation so the cost is per-batch, not
    /// per-edit.
    fn total_bytes(&self) -> usize {
        self.paragraphs.iter().map(|p| p.text.len_bytes()).sum()
    }

    /// Apply a batch of edits. Returns `Some(())` on success and
    /// `None` if the batch failed validation (state unchanged).
    ///
    /// The batch is pre-validated against the doc-wide byte range
    /// invariants; per-edit application happens in REVERSE source
    /// order so each edit's pre-shift offsets stay valid against
    /// the still-pre-edit prefix.
    fn apply_edits(&mut self, edits: &[LocalTextEdit]) -> Option<()> {
        if let Err(err) = self.validate_edits(edits) {
            tracing::warn!(
                error = %err,
                text_bytes = self.total_bytes(),
                "rejecting incremental edit batch; document state unchanged",
            );
            return None;
        }
        for edit in edits.iter().rev() {
            self.apply_one_edit(edit);
        }
        Some(())
    }

    fn validate_edits(&self, edits: &[LocalTextEdit]) -> Result<(), EditError> {
        let len = self.total_bytes();
        let mut prev_end = 0usize;
        for edit in edits {
            let start = edit.range.start;
            let end = edit.range.end;
            if end < start {
                return Err(EditError::InvertedRange { start, end });
            }
            if end > len {
                return Err(EditError::OutOfBounds {
                    start,
                    end,
                    source_len: len,
                });
            }
            if !self.is_char_boundary(start) || !self.is_char_boundary(end) {
                return Err(EditError::NonCharBoundary { start, end });
            }
            if start < prev_end {
                return Err(EditError::UnsortedOrOverlapping {
                    prev_end,
                    next_start: start,
                });
            }
            prev_end = end;
        }
        Ok(())
    }

    fn is_char_boundary(&self, doc_byte: usize) -> bool {
        let total = self.total_bytes();
        if doc_byte == 0 || doc_byte == total {
            return true;
        }
        if doc_byte > total {
            return false;
        }
        let (idx, local) = self.locate_byte(doc_byte);
        let rope = &self.paragraphs[idx].text;
        if local == 0 || local == rope.len_bytes() {
            return true;
        }
        let (chunk, chunk_byte_idx, _, _) = rope.chunk_at_byte(local);
        let in_chunk = local - chunk_byte_idx;
        chunk.is_char_boundary(in_chunk)
    }

    /// Resolve a doc-absolute byte to (`paragraph_idx`, `local_byte`).
    /// Boundary at `paragraphs[i].end == paragraphs[i+1].start`
    /// is reported as the LEFT paragraph (consistent with the
    /// `paragraph_byte_ranges` boundary policy).
    ///
    /// `O(N)` walk over paragraphs (no cumulative-offset cache on
    /// the writer side). At LSP keystroke rates with paragraph
    /// counts in the low hundreds this stays sub-microsecond.
    fn locate_byte(&self, doc_byte: usize) -> (usize, usize) {
        let mut acc = 0usize;
        let last = self.paragraphs.len().saturating_sub(1);
        for (idx, paragraph) in self.paragraphs.iter().enumerate() {
            let len = paragraph.text.len_bytes();
            // `<` so that the boundary at `acc + len` belongs to the
            // LEFT paragraph; the final paragraph also catches `acc +
            // len` exactly via the `idx == last` short-circuit, since
            // there is no rightward paragraph to take ownership of
            // doc_byte == total_bytes.
            if doc_byte < acc + len || idx == last {
                return (idx, doc_byte.saturating_sub(acc));
            }
            acc += len;
        }
        // Unreachable in practice — the `idx == last` arm always
        // matches in the loop above. Kept defensive for the
        // empty-doc case that BufferState::new pre-fills.
        (0, 0)
    }

    fn apply_one_edit(&mut self, edit: &LocalTextEdit) {
        let (start_para, start_local) = self.locate_byte(edit.range.start);
        let (end_para, end_local) = self.locate_byte(edit.range.end);
        if start_para == end_para {
            self.apply_within_paragraph(start_para, start_local, end_local, &edit.new_text);
        } else {
            self.apply_across_paragraphs(
                start_para,
                start_local,
                end_para,
                end_local,
                &edit.new_text,
            );
        }
        self.maybe_resegment_around(start_para);
    }

    fn apply_within_paragraph(
        &mut self,
        idx: usize,
        start_local: usize,
        end_local: usize,
        new_text: &str,
    ) {
        let paragraph = &mut self.paragraphs[idx];
        let start_char = paragraph.text.byte_to_char(start_local);
        let end_char = paragraph.text.byte_to_char(end_local);
        if end_char > start_char {
            paragraph.text.remove(start_char..end_char);
        }
        if !new_text.is_empty() {
            paragraph.text.insert(start_char, new_text);
        }
        let new_end_local = start_local + new_text.len();
        // The `InputEdit`'s byte offsets are paragraph-local — that's
        // what `MutParagraph::apply_edit` expects.
        let ts_edit = input_edit(start_local, end_local, new_end_local);
        paragraph.apply_edit(&mut self.parser, ts_edit);
    }

    /// Cross-paragraph edit: build the merged Rope without
    /// materialising any intermediate `String`s. Re-segment the
    /// merged content and replace `paragraphs[start_para..=end_para]`
    /// with the resulting per-paragraph trees.
    ///
    /// **Why a full reparse over the merged region**: distinguishing
    /// "the boundary `\n\n` was deleted, paragraphs collapse" from
    /// "an edit straddled the boundary but produced the same shape"
    /// requires diffing the segmentation outcome against the prior
    /// shape, then matching trees by something other than tree id
    /// (since both old paragraphs' trees are stale relative to the
    /// new merged content). The per-paragraph reuse path on the
    /// snapshot side already handles "subsequent paragraphs reused
    /// via `Arc::clone` on the unaffected suffix"; this writer-side
    /// reparse pays at most `O(merged_size)`, which for typical
    /// boundary-spanning edits is bounded to ~10 KB.
    fn apply_across_paragraphs(
        &mut self,
        start_para: usize,
        start_local: usize,
        end_para: usize,
        end_local: usize,
        new_text: &str,
    ) {
        // Build the merged Rope by zero-copy `append` of slices from
        // the existing paragraphs' Ropes. The middle `new_text`
        // becomes a tiny owned Rope; everything else stays in
        // structural-share territory.
        let mut merged = Rope::from(self.paragraphs[start_para].text.byte_slice(..start_local));
        if !new_text.is_empty() {
            merged.append(Rope::from(new_text));
        }
        merged.append(Rope::from(
            self.paragraphs[end_para].text.byte_slice(end_local..),
        ));

        let ranges = paragraph_byte_ranges(&merged);
        let mut replacement: Vec<MutParagraph> = ranges
            .into_iter()
            .map(|range| paragraph_from_rope_slice(&merged, range, &mut self.parser))
            .collect();
        if replacement.is_empty() {
            replacement.push(MutParagraph::new(Rope::new()));
        }
        self.paragraphs.splice(start_para..=end_para, replacement);
    }

    /// If the paragraph at `idx` grew past the cap (due to an
    /// in-paragraph insert), re-split it by content and reparse the
    /// resulting pieces. Otherwise no-op.
    fn maybe_resegment_around(&mut self, idx: usize) {
        if idx >= self.paragraphs.len() {
            return;
        }
        let len = self.paragraphs[idx].text.len_bytes();
        if len <= MAX_PARAGRAPH_BYTES {
            return;
        }
        // Re-segment the paragraph's text by paragraph_byte_ranges
        // (will hard-cap at MAX_PARAGRAPH_BYTES).
        let text_rope = std::mem::replace(&mut self.paragraphs[idx].text, Rope::new());
        let ranges = paragraph_byte_ranges(&text_rope);
        if ranges.len() <= 1 {
            // Single-segment result — restore and return; the cap
            // hard-split was a no-op (paragraph is exactly cap-sized).
            self.paragraphs[idx].text = text_rope;
            self.paragraphs[idx].reparse(&mut self.parser);
            return;
        }
        let replacement: Vec<MutParagraph> = ranges
            .into_iter()
            .map(|range| paragraph_from_rope_slice(&text_rope, range, &mut self.parser))
            .collect();
        self.paragraphs.splice(idx..=idx, replacement);
    }

    fn replace(&mut self, new_text: String) {
        let rope = Rope::from(new_text);
        let ranges = paragraph_byte_ranges(&rope);
        let mut paragraphs: Vec<MutParagraph> = ranges
            .into_iter()
            .map(|range| paragraph_from_rope_slice(&rope, range, &mut self.parser))
            .collect();
        if paragraphs.is_empty() {
            paragraphs.push(MutParagraph::new(Rope::new()));
        }
        self.paragraphs = paragraphs;
    }
}

// =====================================================================
// Read side: Snapshot
// =====================================================================

/// Immutable read view of a document. Built from a [`BufferState`]
/// snapshot and atomically swapped into [`DocState::snapshot`]. Reads
/// are wait-free (one `ArcSwap::load_full` + Arc clones).
pub struct Snapshot {
    pub paragraphs: Arc<[Arc<ParagraphSnapshot>]>,
    /// `paragraph_starts[i]` = doc-absolute byte where paragraph `i`
    /// begins. Sorted ascending. Lets handlers binary-search a
    /// doc-absolute offset to a paragraph in `O(log n)`.
    pub paragraph_starts: Arc<[u32]>,
    pub total_bytes: u32,
    pub version: u64,

    // Lazy doc-level materialisations. Each `OnceLock` is populated
    // by the first call to its accessor; subsequent calls within the
    // lifetime of this `Snapshot` return the cached `Arc` for free.
    doc_text: OnceLock<Arc<str>>,
    doc_line_index: OnceLock<Arc<LineIndex>>,
    doc_gaiji_spans: OnceLock<Arc<BTreeMap<u32, Arc<GaijiSpan>>>>,
}

impl std::fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Snapshot")
            .field("version", &self.version)
            .field("paragraphs", &self.paragraphs.len())
            .field("total_bytes", &self.total_bytes)
            .finish_non_exhaustive()
    }
}

impl Snapshot {
    /// Doc-wide concatenated text, materialised on first request and
    /// cached for the rest of this snapshot's lifetime. Handlers that
    /// can iterate paragraphs directly should prefer the per-paragraph
    /// accessor and skip this O(n) materialisation entirely.
    #[must_use]
    pub fn doc_text(&self) -> &Arc<str> {
        self.doc_text.get_or_init(|| {
            let total = self.total_bytes as usize;
            let mut buf = String::with_capacity(total);
            for paragraph in self.paragraphs.iter() {
                buf.push_str(&paragraph.text);
            }
            Arc::from(buf)
        })
    }

    /// Doc-wide line index, lazily materialised. Built by re-scanning
    /// `doc_text()` (forces that materialisation as a side effect).
    #[must_use]
    pub fn doc_line_index(&self) -> &Arc<LineIndex> {
        self.doc_line_index.get_or_init(|| {
            let text = self.doc_text();
            Arc::new(LineIndex::new(text))
        })
    }

    /// Doc-wide gaiji-span store keyed by `start_byte`. Concatenates
    /// each paragraph's pre-extracted spans (whose offsets are
    /// already doc-absolute, see `crate::paragraph`).
    #[must_use]
    pub fn doc_gaiji_spans(&self) -> &Arc<BTreeMap<u32, Arc<GaijiSpan>>> {
        self.doc_gaiji_spans.get_or_init(|| {
            let mut map = BTreeMap::new();
            for paragraph in self.paragraphs.iter() {
                for span in paragraph.gaiji_spans.iter() {
                    map.insert(span.start_byte, Arc::clone(span));
                }
            }
            Arc::new(map)
        })
    }

    /// Find the paragraph index that contains `doc_byte`. Returns
    /// `None` only when the snapshot has zero paragraphs (which we
    /// avoid in practice — empty documents still have one
    /// zero-length paragraph). Boundaries belong to the LEFT
    /// paragraph (consistent with `paragraph_byte_ranges`).
    #[must_use]
    pub fn paragraph_at(&self, doc_byte: usize) -> Option<usize> {
        if self.paragraph_starts.is_empty() {
            return None;
        }
        let target = u32::try_from(doc_byte).unwrap_or(u32::MAX);
        let i = self
            .paragraph_starts
            .partition_point(|&s| s <= target)
            .saturating_sub(1);
        Some(i)
    }
}

fn build_snapshot(buffer: &BufferState, version: u64, prior: &Snapshot) -> Arc<Snapshot> {
    // Per-paragraph rebuild: for each paragraph in the new buffer,
    // try to reuse the prior snapshot's paragraph by tree-id match
    // (cheap: `Arc::clone` if matched, full materialisation if not).
    //
    // This is the hot-path payoff of the paragraph-first model: an
    // edit affecting paragraph K leaves paragraphs ≠ K with the
    // same Tree id, so we Arc::clone N - 1 paragraphs for free and
    // pay materialisation only on paragraph K.
    let mut paragraphs: Vec<Arc<ParagraphSnapshot>> = Vec::with_capacity(buffer.paragraphs.len());
    let mut starts: Vec<u32> = Vec::with_capacity(buffer.paragraphs.len());
    let mut acc: u32 = 0;
    for (idx, paragraph) in buffer.paragraphs.iter().enumerate() {
        starts.push(acc);
        let live_id = paragraph.tree_id();
        let new_start = acc as usize;
        let snap = match prior.paragraphs.get(idx) {
            Some(prior_p)
                if prior_p.tree_id == live_id
                    && prior_p.byte_range.len() == paragraph.text.len_bytes() =>
            {
                // Reuse: `shifted_to` handles both the in-place
                // (pure Arc bump) and shifted (share text/line_index/
                // tree, re-emit gaiji spans) cases internally.
                ParagraphSnapshot::shifted_to(prior_p, new_start)
            }
            _ => Arc::new(build_paragraph_snapshot(paragraph, new_start)),
        };
        let bytes = u32::try_from(paragraph.text.len_bytes()).unwrap_or(u32::MAX);
        acc = acc.saturating_add(bytes);
        paragraphs.push(snap);
    }
    if paragraphs.is_empty() {
        // Defensive: should never happen because BufferState
        // guarantees at least one paragraph, but Snapshot's
        // accessors degrade gracefully if it does.
        paragraphs.push(Arc::new(build_paragraph_snapshot(
            &MutParagraph::new(Rope::new()),
            0,
        )));
        starts.push(0);
    }
    Arc::new(Snapshot {
        paragraphs: paragraphs.into(),
        paragraph_starts: starts.into(),
        total_bytes: acc,
        version,
        doc_text: OnceLock::new(),
        doc_line_index: OnceLock::new(),
        doc_gaiji_spans: OnceLock::new(),
    })
}

fn empty_snapshot() -> Arc<Snapshot> {
    let empty_para = Arc::new(build_paragraph_snapshot(&MutParagraph::new(Rope::new()), 0));
    Arc::new(Snapshot {
        paragraphs: Arc::from(vec![empty_para]),
        paragraph_starts: Arc::from(vec![0u32]),
        total_bytes: 0,
        version: 0,
        doc_text: OnceLock::new(),
        doc_line_index: OnceLock::new(),
        doc_gaiji_spans: OnceLock::new(),
    })
}

// =====================================================================
// DocState orchestrator
// =====================================================================

pub struct DocState {
    buffer: Mutex<BufferState>,
    snapshot: ArcSwap<Snapshot>,
    edit_version: AtomicU64,
    pub metrics: Arc<Metrics>,
}

impl std::fmt::Debug for DocState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocState")
            .field("edit_version", &self.edit_version.load(Ordering::Relaxed))
            .field("snapshot_version", &self.snapshot.load().version)
            .finish_non_exhaustive()
    }
}

impl DocState {
    /// Build a new `DocState` and synchronously compute the initial
    /// snapshot.
    #[must_use]
    pub fn new(text: String) -> Arc<Self> {
        let buffer = BufferState::new(text);
        let initial = build_snapshot(&buffer, 0, &empty_snapshot());
        let state = Arc::new(Self {
            buffer: Mutex::new(buffer),
            snapshot: ArcSwap::from(initial),
            edit_version: AtomicU64::new(0),
            metrics: Arc::new(Metrics::default()),
        });
        state.run_segment_cache_reparse();
        state
    }

    /// Wait-free read of the current snapshot.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.load_full()
    }

    pub fn edit_version(&self) -> u64 {
        self.edit_version.load(Ordering::SeqCst)
    }

    pub fn with_segment_cache<R>(&self, f: impl FnOnce(&SegmentCache) -> R) -> R {
        let buffer = self.buffer.lock();
        f(&buffer.segment_cache)
    }

    pub fn install_diagnostics(&self, diagnostics: Vec<aozora::Diagnostic>) {
        let mut buffer = self.buffer.lock();
        buffer.segment_cache.set_diagnostics(diagnostics);
    }

    /// Apply a batch of edits and ratchet the snapshot.
    pub fn apply_changes(self: &Arc<Self>, edits: &[LocalTextEdit]) -> Option<u64> {
        let new_version = {
            let mut buffer = self.buffer.lock();
            buffer.apply_edits(edits)?;
            self.metrics.record_edit();
            self.edit_version.fetch_add(1, Ordering::SeqCst) + 1
        };
        self.spawn_snapshot_rebuild(new_version);
        Some(new_version)
    }

    /// Replace the buffer wholesale.
    pub fn replace_text(self: &Arc<Self>, new_text: String) -> u64 {
        let new_version = {
            let mut buffer = self.buffer.lock();
            buffer.replace(new_text);
            self.metrics.record_edit();
            self.edit_version.fetch_add(1, Ordering::SeqCst) + 1
        };
        self.spawn_snapshot_rebuild(new_version);
        new_version
    }

    /// Synchronous snapshot rebuild — used by tests and the bg
    /// blocking-pool task body. Holds the buffer mutex briefly to
    /// snapshot the paragraph state, then drops it before doing the
    /// per-paragraph snapshot construction (which only touches text
    /// already snapshot-ed).
    pub fn rebuild_snapshot_now(&self) {
        let prior = self.snapshot.load_full();
        let candidate = {
            let buffer = self.buffer.lock();
            let version = self.edit_version.load(Ordering::SeqCst);
            build_snapshot(&buffer, version, &prior)
        };
        self.install_if_newer(&candidate);
    }

    fn install_if_newer(&self, candidate: &Arc<Snapshot>) -> bool {
        let mut installed = false;
        self.snapshot.rcu(|current| {
            if candidate.version >= current.version {
                installed = true;
                Arc::clone(candidate)
            } else {
                installed = false;
                Arc::clone(current)
            }
        });
        installed
    }

    fn spawn_snapshot_rebuild(self: &Arc<Self>, target_version: u64) {
        let this = Arc::clone(self);
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(move || {
                if this.snapshot.load().version >= target_version {
                    return;
                }
                this.rebuild_snapshot_now();
            });
        } else {
            this.rebuild_snapshot_now();
        }
    }

    pub fn run_segment_cache_reparse(&self) {
        let stats = {
            let mut buffer = self.buffer.lock();
            let mut text = String::new();
            for paragraph in &buffer.paragraphs {
                text.push_str(&paragraph.text.to_string());
            }
            let (_diags, stats) = buffer.segment_cache.reparse(&text);
            stats
        };
        self.metrics.record_parse(
            stats.latency_us,
            stats.cache_hits,
            stats.cache_misses,
            stats.cache_entries_after,
            stats.cache_bytes_estimate,
        );
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

    /// Subset of `Snapshot::paragraph_at` exposed via `&self` for
    /// tests that want to assert routing without holding the buffer
    /// mutex. Reads through `snapshot.load()`.
    #[cfg(test)]
    pub fn paragraph_at_for_test(&self, doc_byte: usize) -> Option<usize> {
        self.snapshot().paragraph_at(doc_byte)
    }
}

fn slow_parse_threshold_us() -> u64 {
    std::env::var("AOZORA_LSP_SLOW_PARSE_US")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(100_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> Arc<DocState> {
        DocState::new(text.to_owned())
    }

    #[test]
    fn new_doc_publishes_initial_snapshot() {
        let state = doc("hello");
        let snap = state.snapshot();
        assert_eq!(&**snap.doc_text(), "hello");
        assert_eq!(snap.version, 0);
        assert!(snap.doc_gaiji_spans().is_empty());
    }

    #[test]
    fn apply_changes_ratchets_edit_version() {
        let state = doc("hello");
        let v = state
            .apply_changes(&[LocalTextEdit::new(5..5, " world".to_owned())])
            .expect("valid edit");
        assert_eq!(v, 1);
        assert_eq!(state.edit_version(), 1);
        let snap = state.snapshot();
        assert_eq!(&**snap.doc_text(), "hello world");
        assert_eq!(snap.version, 1);
    }

    #[test]
    fn replace_text_ratchets_edit_version() {
        let state = doc("hello");
        let v = state.replace_text("world".to_owned());
        assert_eq!(v, 1);
        let snap = state.snapshot();
        assert_eq!(&**snap.doc_text(), "world");
        assert_eq!(snap.version, 1);
    }

    #[test]
    fn rejected_edit_leaves_state_unchanged() {
        let state = doc("あ");
        let edit = LocalTextEdit::new(1..2, String::new());
        assert!(state.apply_changes(&[edit]).is_none());
        let snap = state.snapshot();
        assert_eq!(&**snap.doc_text(), "あ");
        assert_eq!(snap.version, 0);
        assert_eq!(state.edit_version(), 0);
    }

    #[test]
    fn snapshot_loads_are_lock_free_after_install() {
        let state = doc("｜青空《あおぞら》");
        let s1 = state.snapshot();
        let s2 = state.snapshot();
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    #[test]
    fn paragraph_split_on_blank_line() {
        let state = doc("段落1\n\n段落2");
        let snap = state.snapshot();
        assert_eq!(snap.paragraphs.len(), 2, "{snap:?}");
        // Document-absolute first byte of paragraph 1 is right after
        // the blank-line boundary inside paragraph 0.
        assert!(snap.paragraph_starts[1] > 0);
    }

    #[test]
    fn within_paragraph_edit_only_touches_one_paragraph_snapshot() {
        let state = doc("段落1\n\n段落2\n\n段落3");
        let snap_before = state.snapshot();
        assert_eq!(snap_before.paragraphs.len(), 3);
        let para0_before = Arc::clone(&snap_before.paragraphs[0]);
        let text2_before = Arc::clone(&snap_before.paragraphs[2].text);
        let line2_before = Arc::clone(&snap_before.paragraphs[2].line_index);

        // Insert inside paragraph 1.
        let mid_para1 = "段落1\n\n段".len();
        state
            .apply_changes(&[LocalTextEdit::new(mid_para1..mid_para1, "X".to_owned())])
            .unwrap();
        let snap_after = state.snapshot();

        // Paragraph 0 is in-place + unchanged — pure Arc bump (same pointer).
        assert!(Arc::ptr_eq(&snap_after.paragraphs[0], &para0_before));
        // Paragraph 1 is a fresh Arc (its tree was reparsed).
        assert!(!Arc::ptr_eq(
            &snap_after.paragraphs[1],
            &snap_before.paragraphs[1]
        ));
        // Paragraph 2's outer Arc is fresh (because byte_range shifted)
        // BUT the inner text + line_index Arcs ARE shared with the
        // prior snapshot — the only newly-allocated piece is the
        // gaiji-spans list (with shifted offsets) plus the small
        // `Arc<ParagraphSnapshot>` itself.
        assert!(Arc::ptr_eq(&snap_after.paragraphs[2].text, &text2_before));
        assert!(Arc::ptr_eq(
            &snap_after.paragraphs[2].line_index,
            &line2_before
        ));
    }

    #[test]
    fn doc_text_caches_after_first_call() {
        let state = doc("hello\n\nworld");
        let snap = state.snapshot();
        let t1 = snap.doc_text();
        let t2 = snap.doc_text();
        assert!(Arc::ptr_eq(t1, t2), "OnceLock should cache");
        assert_eq!(&**t1, "hello\n\nworld");
    }

    #[test]
    fn paragraph_at_resolves_doc_byte() {
        let state = doc("一\n\n二\n\n三");
        let snap = state.snapshot();
        assert_eq!(snap.paragraph_at(0), Some(0));
        // After the first \n\n, we should be in paragraph 1.
        let after_first_blank = "一\n\n".len();
        assert_eq!(snap.paragraph_at(after_first_blank), Some(1));
    }
}
