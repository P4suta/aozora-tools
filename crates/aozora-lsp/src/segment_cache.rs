//! Per-document parse wrapper for the LSP backend.
//!
//! # Migration note (Phase 0 of the editor-integration sprint)
//!
//! The previous implementation cached per-paragraph parses keyed by
//! content hash, leaning on `aozora_parser::{identify_segments,
//! parse_segment, merge_segments}` to merge them into a whole-document
//! `ParseResult`. The 0.2 split (top-level `aozora` crate, borrowed
//! AST in a bumpalo arena) retired both the segment APIs and the
//! `ParseResult` shape. The new `aozora::Document::parse` is fast
//! enough that the corpus median document re-parses in single-digit
//! milliseconds — well below the keystroke-perceptibility threshold
//! the cache was sized against — so the segment cache is replaced
//! with a straightforward "stash the latest diagnostics; re-derive
//! the parse on demand".
//!
//! # Why no stored `Document`
//!
//! `aozora::Document` owns a `bumpalo::Bump` whose interior `Cell`s
//! make it `!Sync`. The LSP backend wraps every per-document state
//! in `Arc<DashMap<Url, DocState>>`, which requires `DocState: Sync`.
//! Stashing a `Document` inside `DocState` therefore cannot work
//! across threads. Instead, [`SegmentCache`] stores the latest
//! diagnostics and re-parses with a fresh `Document` whenever a
//! request handler needs the [`AozoraTree`].
//!
//! Per-call statistics ([`ReparseStats`]) are still produced so the
//! [`crate::metrics::Metrics`] dashboard keeps the same fields it
//! used in the cache era — `cache_hits` / `cache_misses` are set to
//! `0` / `1` (every reparse is a "miss" by definition under the new
//! whole-document model).

use std::time::{Duration, Instant};

use aozora::{AozoraTree, Diagnostic, Document};
use tracing::field::Empty as TracingEmpty;

/// Per-call statistics emitted by [`SegmentCache::reparse`].
///
/// The caller (typically the LSP backend's `DocState`) feeds these
/// into the per-document [`crate::metrics::Metrics`] so parse latency,
/// segment count, and (legacy) cache hit fields are observable from a
/// third party reading the log.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReparseStats {
    pub segment_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub cache_entries_after: u64,
    pub cache_bytes_estimate: u64,
    pub latency_us: u64,
}

/// Per-document state holder for the LSP backend.
///
/// Keeps the latest diagnostics so the `publishDiagnostics` path can
/// answer in O(1) without re-parsing. Reads needing the
/// [`AozoraTree`] (hover, inlay hints, completion) call
/// [`Self::with_tree`], which builds a fresh [`Document`] on the
/// stack and yields a borrowed tree to the closure.
#[derive(Debug, Default, Clone)]
pub struct SegmentCache {
    /// Latest source text. Owned so reads don't have to borrow back
    /// into the parent `DocState`.
    text: String,
    /// Diagnostics from the most recent [`Self::reparse`]. Empty
    /// until the first parse.
    diagnostics: Vec<Diagnostic>,
}

impl SegmentCache {
    /// Re-parse `text`. Returns the diagnostics produced by the parse
    /// plus per-call statistics.
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(
            text_bytes = text.len(),
            latency_us = TracingEmpty,
        ),
    )]
    pub fn reparse(&mut self, text: &str) -> (Vec<Diagnostic>, ReparseStats) {
        let started_at = Instant::now();
        let document = Document::new(text);
        let diagnostics: Vec<Diagnostic> = document.parse().diagnostics().to_vec();
        let latency_us = duration_as_us(started_at.elapsed());

        text.clone_into(&mut self.text);
        self.diagnostics.clone_from(&diagnostics);

        let bytes_estimate = u64::try_from(text.len()).unwrap_or(u64::MAX);
        let stats = ReparseStats {
            segment_count: 1,
            cache_hits: 0,
            cache_misses: 1,
            cache_entries_after: 1,
            cache_bytes_estimate: bytes_estimate,
            latency_us,
        };
        tracing::Span::current().record("latency_us", latency_us);
        (diagnostics, stats)
    }

    /// Borrow the most recent diagnostics. Empty until the first
    /// successful [`Self::reparse`].
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Install diagnostics produced by an out-of-band parse (Stage 5
    /// debounced background task). Replaces the prior diagnostic
    /// vector wholesale; the caller has already verified that this
    /// parse corresponds to the current text version.
    pub fn set_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics = diagnostics;
    }

    /// Run `f` against a freshly parsed [`AozoraTree`]. Returns the
    /// closure's result, or `None` if no [`Self::reparse`] has been
    /// called yet (text is empty).
    ///
    /// The Document is built on the stack inside this call so its
    /// `!Sync` arena does not leak into the surrounding `DocState`.
    /// Re-parse cost is paid per call; for keystroke-rate UIs the
    /// new bumpalo pipeline absorbs this comfortably (sub-ms median
    /// on the corpus).
    pub fn with_tree<R>(&self, f: impl FnOnce(&AozoraTree<'_>) -> R) -> Option<R> {
        if self.text.is_empty() && self.diagnostics.is_empty() {
            return None;
        }
        let document = Document::new(self.text.as_str());
        let tree = document.parse();
        Some(f(&tree))
    }

    /// Whether any text has been parsed yet.
    #[cfg(test)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_empty() && self.diagnostics.is_empty()
    }
}

/// Convert a `Duration` to whole microseconds, saturating at
/// `u64::MAX`.
fn duration_as_us(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_reparse_populates_state() {
        let mut cache = SegmentCache::default();
        assert!(cache.is_empty());
        let (diags, stats) = cache.reparse("hello, world");
        assert!(diags.is_empty());
        assert_eq!(stats.segment_count, 1);
    }

    #[test]
    fn reparse_updates_text_and_with_tree_sees_it() {
        let mut cache = SegmentCache::default();
        drop(cache.reparse("first"));
        drop(cache.reparse("｜青梅《おうめ》"));
        let inline_count = cache
            .with_tree(|tree| tree.lex_output().registry.inline.len())
            .expect("populated");
        assert_eq!(inline_count, 1);
    }

    #[test]
    fn reparse_reports_latency_micros() {
        let mut cache = SegmentCache::default();
        let (_, stats) = cache.reparse("plain text");
        assert!(stats.latency_us < 10_000_000, "stats: {stats:?}");
    }

    #[test]
    fn pua_collision_surfaces_diagnostic() {
        let mut cache = SegmentCache::default();
        let (diags, _) = cache.reparse("abc\u{E001}def");
        assert!(
            diags
                .iter()
                .any(|d| matches!(d, Diagnostic::SourceContainsPua { .. })),
            "expected SourceContainsPua, got {diags:?}",
        );
    }

    #[test]
    fn empty_text_parses_with_no_diagnostics() {
        let mut cache = SegmentCache::default();
        let (diags, _) = cache.reparse("");
        assert!(diags.is_empty());
    }
}
