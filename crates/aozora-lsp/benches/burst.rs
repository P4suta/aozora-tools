//! Editor-burst microbenchmark.
//!
//! Reproduces the keystroke-burst pattern from the user trace
//! (10:05 PM, samples/bouten.afm 6.3 MB, 108k lines, ~24k notation
//! tokens). Splits into focused micro-benches so we can attribute
//! the wall time to specific paths:
//!
//! - `apply_changes/insert_one_char` — single edit cost on a 6 MB
//!   doc. Under the current architecture this also rebuilds
//!   `LineIndex` and re-walks the tree-sitter tree to refresh
//!   `gaiji_spans`. **Hypothesis: dominated by gaiji-span tree walk.**
//! - `apply_changes/burst_100`            — 100 sequential edits.
//!   Linear in count if no per-edit allocation cliff.
//! - `inlay_solo`              — one `inlay_hints` call against the
//!   pre-extracted span list. Should be sub-millisecond.
//! - `gaiji_span_extract`      — just the tree walk in
//!   `extract_gaiji_spans`. Bounds the cost we'd pay if we
//!   recomputed eagerly.
//! - `line_index_build`        — `LineIndex::new` over 6 MB. Should
//!   be a few ms.
//!
//! Run with `cargo bench -p aozora-lsp --bench burst`.

use std::path::Path;
use std::sync::Arc;

use aozora_lsp::{
    DocState, GaijiSpan, IncrementalDoc, LineIndex, LocalTextEdit, apply_edits, inlay_hints,
    input_edit,
};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tower_lsp::lsp_types::{Position, Range};

fn load_fixture(name: &str) -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // crates/aozora-lsp → ../.. = workspace root
    let path = Path::new(&manifest)
        .join("../..")
        .join("samples")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "fixture {name} not at {}: {err}; copy your bouten/sample into samples/",
            path.display()
        )
    })
}

fn full_range_for(text: &str, idx: &LineIndex) -> Range {
    Range::new(Position::new(0, 0), idx.position(text, text.len()))
}

fn bench_apply_changes(c: &mut Criterion) {
    let text = load_fixture("bouten.afm");
    let mut g = c.benchmark_group("apply_changes");
    g.sample_size(20);
    // NOTE: outside a tokio runtime, `DocState::apply_changes` falls
    // back to a synchronous snapshot rebuild — so this measurement
    // captures the full end-to-end cost (buffer mutate + TS apply_edit
    // + snapshot rebuild). In production the rebuild runs on the
    // tokio blocking pool and `apply_changes` returns in microseconds;
    // we measure the worst-case wall here on purpose.
    g.bench_function("insert_one_char_bouten_6mb", |b| {
        b.iter_batched(
            || DocState::new(text.clone()),
            |state| {
                let _ = state.apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())]);
            },
            BatchSize::PerIteration,
        );
    });
    g.bench_function("burst_100_inserts_bouten_6mb", |b| {
        b.iter_batched(
            || DocState::new(text.clone()),
            |state| {
                for _ in 0..100 {
                    let _ = state.apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())]);
                }
            },
            BatchSize::PerIteration,
        );
    });

    // Mid-document edit — exercises the **incremental** snapshot
    // rebuild path. Insert one space at a UTF-8 boundary near the
    // doc midpoint; tree-sitter's `changed_ranges` should localise
    // the work to a small region around the cursor, dropping the
    // gaiji-span re-walk from 67 ms (cold) down to a few hundred μs
    // (carry-forward + sub-walk only).
    g.bench_function("insert_one_char_mid_doc_bouten_6mb", |b| {
        let mid_offset = nearest_char_boundary(&text, text.len() / 2);
        b.iter_batched(
            || DocState::new(text.clone()),
            move |state| {
                let _ = state
                    .apply_changes(&[LocalTextEdit::new(mid_offset..mid_offset, " ".to_owned())]);
            },
            BatchSize::PerIteration,
        );
    });
    g.finish();
}

/// Find the nearest UTF-8 char boundary at or before `target` so we
/// can construct a valid mid-document edit range without triggering
/// `apply_edits`'s `NonCharBoundary` rejection.
fn nearest_char_boundary(text: &str, target: usize) -> usize {
    let mut idx = target.min(text.len());
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn bench_inlay(c: &mut Criterion) {
    let text = load_fixture("bouten.afm");
    let state = DocState::new(text.clone());
    let snap = state.snapshot();
    // `inlay_hints` (the public library helper for editors that prefer
    // server-side inlay) takes a sorted slice — collect from the
    // snapshot's BTreeMap once. Production reads use the BTreeMap
    // directly via `Snapshot::gaiji_spans.values()`; this bench is the
    // only consumer of the slice form.
    let spans: Vec<Arc<GaijiSpan>> = snap.doc_gaiji_spans().values().cloned().collect();
    let range = full_range_for(snap.doc_text(), snap.doc_line_index());
    let mut g = c.benchmark_group("inlay");
    g.bench_function("solo_full_range_bouten_6mb", |b| {
        b.iter(|| {
            let _ = inlay_hints(snap.doc_text(), &spans, snap.doc_line_index(), range);
        });
    });
    g.finish();
}

#[allow(
    clippy::too_many_lines,
    reason = "one cohesive criterion bench group; splitting per-bench helpers spreads the same loop pattern across multiple fns without clarifying anything"
)]
fn bench_subcomponents(c: &mut Criterion) {
    let text = load_fixture("bouten.afm");
    let mut g = c.benchmark_group("subcomponents");
    g.sample_size(20);

    g.bench_function("line_index_build_bouten_6mb", |b| {
        b.iter(|| {
            let idx = LineIndex::new(&text);
            std::hint::black_box(idx);
        });
    });

    g.bench_function("gaiji_span_extract_bouten_6mb", |b| {
        b.iter_batched(
            || {
                let doc = IncrementalDoc::new();
                doc.parse_full(&text);
                doc
            },
            |doc| {
                let spans = doc
                    .with_tree(|tree| aozora_lsp::extract_gaiji_spans_for_bench(tree, &text))
                    .unwrap_or_else(|| Arc::from(Vec::new()));
                std::hint::black_box(spans);
            },
            BatchSize::PerIteration,
        );
    });

    g.bench_function("ts_parse_full_bouten_6mb", |b| {
        b.iter_batched(
            IncrementalDoc::new,
            |doc| {
                doc.parse_full(&text);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });

    // Isolate the pure string splice — `apply_changes` flow does this
    // first, and we suspect it's the dominant cost on a 6 MB buffer
    // because `apply_edits` allocates a fresh `String::with_capacity`
    // and `push_str`s the entire prefix + tail every edit.
    g.bench_function("apply_edits_insert_one_char_bouten_6mb", |b| {
        let edit = vec![LocalTextEdit::new(0..0, " ".to_owned())];
        b.iter(|| {
            let new_text = apply_edits(&text, &edit).expect("valid edit");
            std::hint::black_box(new_text);
        });
    });

    // Isolate the tree-sitter incremental edit at OFFSET 0 — the
    // worst case for incremental reuse because every byte after the
    // edit shifts. tree-sitter must invalidate (almost) every node.
    g.bench_function("ts_apply_edit_offset_0_bouten_6mb", |b| {
        b.iter_batched(
            || {
                let doc = IncrementalDoc::new();
                doc.parse_full(&text);
                doc
            },
            |doc| {
                let edit = input_edit(0, 0, 1);
                let mut new_text = String::with_capacity(text.len() + 1);
                new_text.push(' ');
                new_text.push_str(&text);
                doc.apply_edit(&new_text, edit);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });

    // Same benchmark but the edit is in the middle of the document.
    // Tree-sitter's incremental reparse should reuse most subtrees;
    // the cost should drop from ~200 ms (offset-0 worst case) down to
    // a fraction. Exact ratio is the whole point of the measurement.
    g.bench_function("ts_apply_edit_mid_doc_bouten_6mb", |b| {
        let mid = nearest_char_boundary(&text, text.len() / 2);
        let text_for_setup = text.clone();
        let text_for_run = text.clone();
        b.iter_batched(
            move || {
                let doc = IncrementalDoc::new();
                doc.parse_full(&text_for_setup);
                doc
            },
            |doc| {
                let edit = input_edit(mid, mid, mid + 1);
                let mut new_text = String::with_capacity(text_for_run.len() + 1);
                new_text.push_str(&text_for_run[..mid]);
                new_text.push(' ');
                new_text.push_str(&text_for_run[mid..]);
                doc.apply_edit(&new_text, edit);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });

    // Cold parse on a small sub-slice of the doc — this is the
    // measurement that motivates per-paragraph segmentation. If a
    // 60 KB paragraph parses in ~3 ms, then a per-paragraph design
    // turns the per-edit TS cost from 220 ms into 3 ms.
    g.bench_function("ts_parse_full_60kb_slice_bouten", |b| {
        let slice_end = nearest_char_boundary(&text, 60 * 1024);
        let small = &text[..slice_end];
        b.iter_batched(
            IncrementalDoc::new,
            |doc| {
                doc.parse_full(small);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });
    g.bench_function("ts_parse_full_600kb_slice_bouten", |b| {
        let slice_end = nearest_char_boundary(&text, 600 * 1024);
        let small = &text[..slice_end];
        b.iter_batched(
            IncrementalDoc::new,
            |doc| {
                doc.parse_full(small);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });

    g.finish();
}

/// Quantify the wait-free read property of the `ArcSwap`-backed
/// snapshot. Two measurements:
///
/// - `snapshot_load_solo` — `state.snapshot()` against a quiescent
///   `DocState`. Should be sub-microsecond (single atomic load + Arc
///   bump).
/// - `snapshot_load_under_write_pressure` — same call while a
///   background thread hammers `apply_changes` on the same state.
///   The architectural claim is "reads never wait on writers", so
///   the per-call latency must remain sub-microsecond — same order
///   of magnitude as the solo case.
///
/// If the solo and under-pressure numbers diverge, the wait-free
/// invariant is broken (the snapshot pointer is being held live
/// somewhere that contends with writers). The bench is a regression
/// gate for the refactor.
fn bench_concurrent_reads(c: &mut Criterion) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    let text = load_fixture("bouten.afm");
    let mut g = c.benchmark_group("concurrent_reads");
    g.sample_size(20);

    g.bench_function("snapshot_load_solo_bouten_6mb", |b| {
        let state = DocState::new(text.clone());
        b.iter(|| {
            let snap = state.snapshot();
            std::hint::black_box(snap);
        });
    });

    g.bench_function("snapshot_load_under_write_pressure_bouten_6mb", |b| {
        let state = DocState::new(text.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let writer_state = Arc::clone(&state);
        let writer_stop = Arc::clone(&stop);
        // Spawn a writer thread that loops `apply_changes`. The
        // synchronous fall-back inside DocState::apply_changes means
        // each write spends ~270 ms holding the buffer mutex; reads
        // hitting the snapshot must be unaffected.
        let writer = thread::spawn(move || {
            let mut i = 0usize;
            while !writer_stop.load(Ordering::Relaxed) {
                let _ = writer_state.apply_changes(&[LocalTextEdit::new(i..i, " ".to_owned())]);
                i += 2; // Skip ahead each round to avoid edits stacking on each other
            }
        });
        b.iter(|| {
            let snap = state.snapshot();
            std::hint::black_box(snap);
        });
        stop.store(true, Ordering::Relaxed);
        writer.join().expect("writer thread joined");
    });

    g.finish();
}

criterion_group!(
    benches,
    bench_subcomponents,
    bench_apply_changes,
    bench_inlay,
    bench_concurrent_reads
);
criterion_main!(benches);
