//! Per-document state: a mutable `BufferState` behind a single
//! `parking_lot::Mutex`, plus an immutable [`Snapshot`] swapped
//! atomically into an [`ArcSwap`] for wait-free reads.
//!
//! ## Why this shape
//!
//! The earlier `DocState` carried `text`, `line_index`, `gaiji_spans`,
//! `incremental`, `segment_cache`, and `parse_version` together inside
//! a single `DashMap` value. Every LSP request that read any of those
//! fields blocked behind the same `dashmap::RwLock` shard that
//! `did_change` held while applying a 200 ms tree-sitter incremental
//! reparse on a 6 MB document. The user-observed effect was 1–2 second
//! inlay/diagnostic lag during keystroke bursts on large docs
//! (criterion `apply_changes/insert_one_char_bouten_6mb` measured
//! 267 ms per edit on `samples/bouten.afm` — 200 ms tree-sitter
//! reparse, 70 ms gaiji span extract, 3 ms `LineIndex` rebuild,
//! 0.3 ms string splice).
//!
//! The split below decouples the read and write paths entirely:
//!
//! - [`BufferState`] (writers only): `String`, `IncrementalDoc`,
//!   `SegmentCache`. Held behind one `parking_lot::Mutex`. Mutated on
//!   `apply_changes` / `replace_text`. Concurrent writers (rare in
//!   single-user editing — VS Code serialises `did_change` per doc)
//!   wait on each other; readers never wait on this mutex.
//! - [`Snapshot`] (readers only): `Arc<str>` text, `Arc<LineIndex>`,
//!   `Arc<BTreeMap<u32, GaijiSpan>>`, `version`. The store is a
//!   `BTreeMap` rather than a sorted `Vec` so a future incremental
//!   rebuild (only re-walk `Tree::changed_ranges` between old/new
//!   trees, shift the rest by the edit delta) is `O(log n + k)` rather
//!   than `O(n)`. Today the rebuild is still full-walk; the data
//!   structure is the load-bearing choice.
//! - [`DocState`] orchestrates the two halves. `edit_version`
//!   monotonically counts applied edits; `Snapshot::version` records
//!   "buffer this snapshot was built from". A bg `spawn_blocking` task
//!   ratchets the snapshot forward; `ArcSwap::rcu` ensures only the
//!   freshest install wins.
//!
//! ## Read path
//!
//! ```ignore
//! let snapshot = entry.snapshot(); // ArcSwap::load_full -> Arc<Snapshot>
//! let text     = &snapshot.text;
//! let line_idx = &snapshot.line_index;
//! ```
//!
//! Wait-free: a single atomic load and an `Arc` clone. No mutex, no
//! dashmap shard upgrade, no parser invocation.
//!
//! ## Write path
//!
//! `apply_changes` acquires the buffer mutex, splices the text,
//! applies the tree-sitter incremental edit, bumps `edit_version`,
//! drops the mutex, and spawns a blocking task to rebuild the
//! snapshot. The handler returns immediately after the synchronous
//! buffer mutation; the snapshot lags by one rebuild and catches up
//! within ~270 ms on the worst-case 6 MB document.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use tree_sitter::{InputEdit, Tree};

use crate::gaiji_spans::{GaijiSpan, extract_gaiji_spans, extract_gaiji_spans_incremental};
use crate::incremental::{IncrementalDoc, input_edit};
use crate::line_index::LineIndex;
use crate::metrics::Metrics;
use crate::segment_cache::SegmentCache;
use crate::text_edit::{LocalTextEdit, apply_edits};

/// Mutable side of the per-document state. Held behind
/// `DocState::buffer`. Writers (`apply_changes`, `replace_text`,
/// segment-cache reparse) acquire this mutex briefly; readers never
/// touch it.
#[derive(Debug)]
pub struct BufferState {
    pub text: String,
    pub incremental: IncrementalDoc,
    pub segment_cache: SegmentCache,
}

impl BufferState {
    fn new(text: String) -> Self {
        let incremental = IncrementalDoc::new();
        incremental.parse_full(&text);
        Self {
            text,
            incremental,
            segment_cache: SegmentCache::default(),
        }
    }

    /// Apply a batch of edits to `text` and propagate the edits into
    /// the tree-sitter incremental tree. Returns the list of
    /// `InputEdit`s applied (for the snapshot rebuild's incremental
    /// gaiji-span shift), or `None` if validation rejected the batch
    /// (text and tree remain at the prior state).
    fn apply_edits(&mut self, edits: &[LocalTextEdit]) -> Option<Vec<InputEdit>> {
        // Snapshot byte ranges BEFORE mutating `self.text` so the
        // tree-sitter `InputEdit` references pre-change offsets.
        let ts_edits: Vec<InputEdit> = edits
            .iter()
            .map(|e| input_edit(e.range.start, e.range.end, e.range.start + e.new_text.len()))
            .collect();
        match apply_edits(&self.text, edits) {
            Ok(new_text) => {
                self.text = new_text;
                for edit in &ts_edits {
                    self.incremental.apply_edit(&self.text, *edit);
                }
                Some(ts_edits)
            }
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    text_bytes = self.text.len(),
                    "rejecting incremental edit batch; document state unchanged",
                );
                None
            }
        }
    }

    fn replace(&mut self, new_text: String) {
        self.text = new_text;
        self.incremental.parse_full(&self.text);
    }

    /// Snapshot the buffer for off-mutex snapshot computation. Returns
    /// an `Arc<str>` clone of the text and a cheap shallow `Tree`
    /// clone (tree-sitter `Tree` is internally `Arc`-shared).
    fn snapshot_inputs(&self) -> (Arc<str>, Option<Tree>) {
        let text: Arc<str> = Arc::from(self.text.as_str());
        let tree = self.incremental.with_tree(Clone::clone);
        (text, tree)
    }
}

/// Immutable read view of a document. Built from a [`BufferState`]
/// snapshot and atomically swapped into [`DocState::snapshot`]. Every
/// LSP request handler reads one of these via [`DocState::snapshot`];
/// reads are wait-free.
#[derive(Debug)]
pub struct Snapshot {
    pub text: Arc<str>,
    pub line_index: Arc<LineIndex>,
    /// Sorted gaiji span store keyed by `start_byte`. `BTreeMap`
    /// rather than `Vec` so the incremental rebuild can update only
    /// the spans intersecting a tree-sitter `changed_range` —
    /// `O(log n + k)` instead of `O(n)` full re-walk.
    pub gaiji_spans: Arc<BTreeMap<u32, GaijiSpan>>,
    /// The tree-sitter tree this snapshot was built from. Held here
    /// (cheap shallow `Arc` clone) so the next incremental rebuild
    /// can call `Tree::changed_ranges(&old_tree, &new_tree)` to
    /// localise the work to changed bytes.
    pub tree: Option<Tree>,
    /// `DocState::edit_version` value this snapshot was computed
    /// from. May lag the live version by one rebuild while typing
    /// bursts catch up.
    pub version: u64,
}

impl Snapshot {
    /// Cold-start snapshot: full extract via [`extract_gaiji_spans`].
    /// Used by [`DocState::new`] and as the fallback when no prior
    /// snapshot exists for the incremental algorithm.
    fn build_cold(text: Arc<str>, tree: Option<Tree>, version: u64) -> Self {
        let line_index = Arc::new(LineIndex::new(&text));
        let gaiji_spans = tree
            .as_ref()
            .map(|t| spans_to_btree(&extract_gaiji_spans(t, &text)))
            .unwrap_or_default();
        Self {
            text,
            line_index,
            gaiji_spans: Arc::new(gaiji_spans),
            tree,
            version,
        }
    }

    /// Incremental snapshot: reuse spans from `prior` whose byte
    /// range doesn't intersect the changed regions between the prior
    /// tree and `new_tree`, walking only the changed regions for
    /// fresh extraction.
    fn build_incremental(
        text: Arc<str>,
        new_tree: Tree,
        prior: &Snapshot,
        edits: &[InputEdit],
        version: u64,
    ) -> Self {
        let line_index = Arc::new(LineIndex::new(&text));
        let gaiji_spans = match prior.tree.as_ref() {
            Some(old_tree) => extract_gaiji_spans_incremental(
                old_tree,
                &new_tree,
                &prior.gaiji_spans,
                edits,
                &text,
            ),
            None => spans_to_btree(&extract_gaiji_spans(&new_tree, &text)),
        };
        Self {
            text,
            line_index,
            gaiji_spans: Arc::new(gaiji_spans),
            tree: Some(new_tree),
            version,
        }
    }
}

fn spans_to_btree(spans: &[GaijiSpan]) -> BTreeMap<u32, GaijiSpan> {
    // `extract_gaiji_spans` returns spans in source order; collect
    // into a BTreeMap keyed by start_byte. Duplicate start_byte values
    // would clobber each other but the tree-sitter walker emits each
    // gaiji node once, so there are no duplicates by construction.
    spans.iter().map(|s| (s.start_byte, s.clone())).collect()
}

/// Per-document orchestrator. Holds the [`BufferState`] mutex on the
/// write path and an [`ArcSwap`] [`Snapshot`] on the read path; the
/// two halves are kept in sync via `edit_version` ratcheting.
pub struct DocState {
    buffer: Mutex<BufferState>,
    snapshot: ArcSwap<Snapshot>,
    /// Monotonically incremented under the buffer mutex on every
    /// successful edit. `Snapshot::version` records the value this
    /// snapshot was built from; a bg task observes the lag and
    /// rebuilds.
    edit_version: AtomicU64,
    /// Per-edit `InputEdit` log, tagged with the edit version.
    /// Pushed under the buffer mutex on every accepted edit. The
    /// snapshot rebuild reads the entries with version above the
    /// prior snapshot's, then prunes them after a successful install.
    /// On a failed install (RCU lost to a newer snapshot) the entries
    /// stay so the next rebuild still sees them.
    pending_edits: Mutex<Vec<(u64, InputEdit)>>,
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
    /// snapshot. The caller (`did_open`) is willing to pay the
    /// startup cost (~270 ms on a 6 MB doc) once; subsequent edits
    /// use the lazy bg-rebuild path.
    #[must_use]
    pub fn new(text: String) -> Arc<Self> {
        let buffer = BufferState::new(text);
        let (text_arc, tree) = buffer.snapshot_inputs();
        let initial_snapshot = Snapshot::build_cold(text_arc, tree, 0);
        let state = Arc::new(Self {
            buffer: Mutex::new(buffer),
            snapshot: ArcSwap::from_pointee(initial_snapshot),
            edit_version: AtomicU64::new(0),
            pending_edits: Mutex::new(Vec::new()),
            metrics: Arc::new(Metrics::default()),
        });
        // Run the segment-cache reparse so didOpen returns ready for
        // the first publishDiagnostics. Same observability fields as
        // the debounced path.
        state.run_segment_cache_reparse();
        state
    }

    /// Wait-free read of the current snapshot. Returns a fresh
    /// `Arc<Snapshot>` whose contents are guaranteed immutable for
    /// the lifetime of the returned `Arc`.
    #[must_use]
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.snapshot.load_full()
    }

    /// Latest edit version observed on the buffer. May be ahead of
    /// `self.snapshot().version` by one in-flight rebuild.
    pub fn edit_version(&self) -> u64 {
        self.edit_version.load(Ordering::SeqCst)
    }

    /// Borrow the segment cache's diagnostics under the buffer mutex.
    /// Reads only — the caller cannot mutate the cache.
    pub fn with_segment_cache<R>(&self, f: impl FnOnce(&SegmentCache) -> R) -> R {
        let buffer = self.buffer.lock();
        f(&buffer.segment_cache)
    }

    /// Install diagnostics into the segment cache from a debounced
    /// background parse. Held briefly under the buffer mutex.
    pub fn install_diagnostics(&self, diagnostics: Vec<aozora::Diagnostic>) {
        let mut buffer = self.buffer.lock();
        buffer.segment_cache.set_diagnostics(diagnostics);
    }

    /// Apply a batch of edits and ratchet the snapshot. Synchronous
    /// path: buffer mutation + tree-sitter incremental edit only;
    /// the heavy snapshot rebuild (line index, gaiji span walk) runs
    /// on the tokio blocking pool so request handlers don't wait.
    ///
    /// Returns the new edit version on success, `None` if the batch
    /// failed validation.
    pub fn apply_changes(self: &Arc<Self>, edits: &[LocalTextEdit]) -> Option<u64> {
        let new_version = {
            let mut buffer = self.buffer.lock();
            let ts_edits = buffer.apply_edits(edits)?;
            self.metrics.record_edit();
            // fetch_add returns the prior value — we want the post.
            let v = self.edit_version.fetch_add(1, Ordering::SeqCst) + 1;
            // Record edits in the pending log under the same lock so
            // the version they're tagged with strictly matches the
            // buffer mutation order.
            let mut log = self.pending_edits.lock();
            for edit in ts_edits {
                log.push((v, edit));
            }
            v
        };
        self.spawn_snapshot_rebuild(new_version);
        Some(new_version)
    }

    /// Replace the buffer wholesale. Same ratcheting as
    /// [`Self::apply_changes`]. A full replacement invalidates every
    /// gaiji span, so the pending edit log is cleared (any edits
    /// before this replacement are now meaningless against the new
    /// buffer state) and the snapshot will rebuild via the cold path.
    pub fn replace_text(self: &Arc<Self>, new_text: String) -> u64 {
        let new_version = {
            let mut buffer = self.buffer.lock();
            buffer.replace(new_text);
            self.metrics.record_edit();
            let v = self.edit_version.fetch_add(1, Ordering::SeqCst) + 1;
            // Clear pending edit log — replace_text breaks the
            // incremental algorithm's invariants (no input-edit chain
            // from old text to new). Cold rebuild will follow.
            self.pending_edits.lock().clear();
            v
        };
        self.spawn_snapshot_rebuild(new_version);
        new_version
    }

    /// Synchronous snapshot rebuild — used by tests and by the
    /// bg-spawned task body. Computes the snapshot OFF the buffer
    /// mutex (only acquires it briefly to clone the text + tree),
    /// then `ArcSwap::rcu`s it into place if our version is at
    /// least as fresh as what's already there. Older parallel
    /// rebuilds lose the race silently.
    ///
    /// Picks the **incremental** rebuild path when both the prior
    /// snapshot has a tree and the pending edit log covers the
    /// version gap; falls back to the cold path otherwise. The
    /// incremental path uses [`Snapshot::build_incremental`] which
    /// only walks the tree-sitter `changed_ranges` for fresh gaiji
    /// extraction and shifts the rest by the cumulative edit delta.
    pub fn rebuild_snapshot_now(&self) {
        let (text, new_tree, version) = {
            let buffer = self.buffer.lock();
            let (text, tree) = buffer.snapshot_inputs();
            let version = self.edit_version.load(Ordering::SeqCst);
            (text, tree, version)
        };

        let prior = self.snapshot.load_full();
        let candidate = match new_tree {
            Some(new_tree) if prior.tree.is_some() && version > prior.version => {
                // Collect edits since the prior snapshot's version.
                let edits: Vec<InputEdit> = {
                    let log = self.pending_edits.lock();
                    log.iter()
                        .filter(|(v, _)| *v > prior.version && *v <= version)
                        .map(|(_, e)| *e)
                        .collect()
                };
                Arc::new(Snapshot::build_incremental(
                    text, new_tree, &prior, &edits, version,
                ))
            }
            other_tree => Arc::new(Snapshot::build_cold(text, other_tree, version)),
        };

        if self.install_if_newer(&candidate) {
            // Successful install — drop edits up to the installed
            // version. Drops the prefix only; later edits (from
            // writes that happened during this rebuild) stay so the
            // next rebuild can apply them incrementally.
            let mut log = self.pending_edits.lock();
            log.retain(|(v, _)| *v > candidate.version);
        }
    }

    fn install_if_newer(&self, candidate: &Arc<Snapshot>) -> bool {
        // RCU loop: install our candidate iff its version is at least
        // as fresh as the current snapshot's. ArcSwap retries on
        // contention so concurrent installers don't lose data.
        // Returns true iff the candidate ended up installed.
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
        // CPU-bound work (LineIndex SIMD scan + tree walk) — runs on
        // the tokio blocking pool so the request handler hot path
        // returns immediately. If we're not inside a tokio runtime
        // (e.g. tests calling `apply_changes` directly), fall back to
        // synchronous rebuild — tests want determinism.
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::task::spawn_blocking(move || {
                // Skip if a newer rebuild already won.
                if this.snapshot.load().version >= target_version {
                    return;
                }
                this.rebuild_snapshot_now();
            });
        } else {
            this.rebuild_snapshot_now();
        }
    }

    /// Run the slow Rust semantic parse through the segment cache and
    /// feed the per-call stats into the per-document `Metrics`.
    /// Centralised so every reparse path (open, replace, debounced
    /// post-edit) records the same observability fields. Held briefly
    /// under the buffer mutex.
    pub fn run_segment_cache_reparse(&self) {
        let stats = {
            let mut buffer = self.buffer.lock();
            let BufferState {
                text,
                segment_cache,
                ..
            } = &mut *buffer;
            let (_diags, stats) = segment_cache.reparse(text);
            stats
        };
        self.metrics.record_parse(
            stats.latency_us,
            stats.cache_hits,
            stats.cache_misses,
            stats.cache_entries_after,
            stats.cache_bytes_estimate,
        );
        // Slow-path WARN. Same threshold convention as the debounced
        // publish path so observers see one consistent signal.
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

    /// Test-only: synchronous text snapshot. Equivalent to
    /// `self.snapshot().text.clone()` but with explicit naming for
    /// test assertions.
    #[cfg(test)]
    #[must_use]
    pub fn text_for_test(&self) -> String {
        self.snapshot().text.to_string()
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
        assert_eq!(&*snap.text, "hello");
        assert_eq!(snap.version, 0);
        assert!(snap.gaiji_spans.is_empty(), "no gaiji in plain text");
    }

    #[test]
    fn apply_changes_ratchets_edit_version() {
        let state = doc("hello");
        let v = state
            .apply_changes(&[LocalTextEdit::new(5..5, " world".to_owned())])
            .expect("valid edit");
        assert_eq!(v, 1);
        assert_eq!(state.edit_version(), 1);
        // Outside a tokio runtime the rebuild is synchronous, so the
        // snapshot is up-to-date by the time we look at it.
        let snap = state.snapshot();
        assert_eq!(&*snap.text, "hello world");
        assert_eq!(snap.version, 1);
    }

    #[test]
    fn replace_text_ratchets_edit_version() {
        let state = doc("hello");
        let v = state.replace_text("world".to_owned());
        assert_eq!(v, 1);
        let snap = state.snapshot();
        assert_eq!(&*snap.text, "world");
        assert_eq!(snap.version, 1);
    }

    #[test]
    fn rejected_edit_leaves_state_unchanged() {
        let state = doc("あ");
        // 1..2 sits inside the 3-byte UTF-8 「あ」 — apply_edits will
        // reject it.
        let edit = LocalTextEdit::new(1..2, String::new());
        assert!(state.apply_changes(&[edit]).is_none());
        let snap = state.snapshot();
        assert_eq!(&*snap.text, "あ");
        assert_eq!(snap.version, 0);
        assert_eq!(state.edit_version(), 0);
    }

    #[test]
    fn snapshot_loads_are_lock_free_after_install() {
        let state = doc("｜青空《あおぞら》");
        let s1 = state.snapshot();
        let s2 = state.snapshot();
        // Two `Arc<Snapshot>` clones should point at the same
        // allocation (no churn between back-to-back loads).
        assert!(Arc::ptr_eq(&s1, &s2));
    }

    #[test]
    fn install_if_newer_rejects_stale_snapshots() {
        let state = doc("hello");
        // Force the snapshot version to 5 by issuing 5 edits.
        for i in 1..=5 {
            state
                .apply_changes(&[LocalTextEdit::new(i..i, "x".to_owned())])
                .unwrap();
        }
        let snap_before = state.snapshot();
        assert_eq!(snap_before.version, 5);
        // Build a stale snapshot by hand and try to install it.
        let stale_snap = Arc::new(Snapshot {
            text: Arc::from("STALE"),
            line_index: Arc::new(LineIndex::new("STALE")),
            gaiji_spans: Arc::new(BTreeMap::new()),
            tree: None,
            version: 3,
        });
        state.install_if_newer(&stale_snap);
        let snap_after = state.snapshot();
        assert_eq!(
            &*snap_after.text, &*snap_before.text,
            "stale install must not displace newer snapshot"
        );
    }

    #[test]
    fn install_if_newer_accepts_equal_version_snapshots() {
        // RCU `>=` lets a re-computed snapshot at the same version
        // overwrite — useful when two rebuilds race and the second
        // arrives moments later. Older readers already returned via
        // load_full() so they're safe.
        let state = doc("hello");
        let snap = state.snapshot();
        let replacement = Arc::new(Snapshot {
            text: Arc::clone(&snap.text),
            line_index: Arc::clone(&snap.line_index),
            gaiji_spans: Arc::clone(&snap.gaiji_spans),
            tree: snap.tree.clone(),
            version: snap.version,
        });
        state.install_if_newer(&replacement);
        let snap_after = state.snapshot();
        assert!(Arc::ptr_eq(&snap_after, &replacement));
    }
}
