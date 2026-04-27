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
    DocState, IncrementalDoc, LineIndex, LocalTextEdit, apply_edits, inlay_hints, input_edit,
};
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tower_lsp::lsp_types::{Position, Range};

fn load_fixture(name: &str) -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // crates/aozora-lsp → ../.. = workspace root
    let path = Path::new(&manifest).join("../..").join("samples").join(name);
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
    g.bench_function("insert_one_char_bouten_6mb", |b| {
        b.iter_batched(
            || DocState::new(text.clone()),
            |mut state| {
                state.apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())]);
            },
            BatchSize::PerIteration,
        );
    });
    g.bench_function("burst_100_inserts_bouten_6mb", |b| {
        b.iter_batched(
            || DocState::new(text.clone()),
            |mut state| {
                for _ in 0..100 {
                    state.apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())]);
                }
            },
            BatchSize::PerIteration,
        );
    });
    g.finish();
}

fn bench_inlay(c: &mut Criterion) {
    let text = load_fixture("bouten.afm");
    let state = DocState::new(text.clone());
    let range = full_range_for(&state.text, &state.line_index);
    let mut g = c.benchmark_group("inlay");
    g.bench_function("solo_full_range_bouten_6mb", |b| {
        b.iter(|| {
            let _ = inlay_hints(&state.text, &state.gaiji_spans, &state.line_index, range);
        });
    });
    g.finish();
}

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

    // Isolate the tree-sitter incremental edit — should be O(small)
    // because TS only marks the changed range; the next `with_tree`
    // parse is incremental.
    g.bench_function("ts_apply_edit_one_char_bouten_6mb", |b| {
        b.iter_batched(
            || {
                let doc = IncrementalDoc::new();
                doc.parse_full(&text);
                doc
            },
            |doc| {
                let edit = input_edit(0, 0, 1);
                // Pre-mutate text to mirror the apply_changes contract:
                // the buffer passed to apply_edit is the POST-change
                // text but the InputEdit references pre-change offsets.
                let mut new_text = String::with_capacity(text.len() + 1);
                new_text.push(' ');
                new_text.push_str(&text);
                doc.apply_edit(&new_text, edit);
                std::hint::black_box(doc);
            },
            BatchSize::PerIteration,
        );
    });

    g.finish();
}

criterion_group!(benches, bench_subcomponents, bench_apply_changes, bench_inlay);
criterion_main!(benches);
