//! One-shot wall-time measurement of the incremental snapshot
//! rebuild on a real-world large document.
//!
//! Loads `samples/bouten.afm` (6.3 MB, 24 k gaiji), opens a
//! `DocState`, and times two single-character edits at different
//! positions to compare cold vs incremental rebuild paths:
//!
//! - Edit 1 (cold path)            — at offset 0, every byte shifts.
//!   Tree-sitter's `changed_ranges` covers the whole doc, so the
//!   incremental algorithm degenerates to full re-walk.
//! - Edit 2 (incremental fast path) — at the doc midpoint, on a UTF-8
//!   boundary. Only a small region's gaiji spans need re-extraction;
//!   the surrounding ~24 k spans pass through with cumulative-delta
//!   shift.
//!
//! Run with:
//! ```text
//! cargo run -p aozora-lsp --release --example measure_incremental
//! ```

use std::path::Path;
use std::time::Instant;

use aozora_lsp::{DocState, LocalTextEdit};

/// Synthesise a gaiji-rich document by repeating a `※[#…]` block.
/// `count` blocks → roughly `count * 60` bytes of gaiji content +
/// surrounding plain prose. Keeps tree-sitter's `gaiji` rule
/// exercised so the incremental algorithm has spans to carry forward.
fn synth_gaiji_doc(count: usize) -> String {
    let mut s = String::with_capacity(count * 80);
    for i in 0..count {
        s.push('第');
        s.push_str(&i.to_string());
        s.push_str("章 一行の文章です。");
        s.push_str("※［＃「あ」、第3水準1-85-54］という外字。");
        s.push('\n');
    }
    s
}

/// Bytes → MB with no precision drama for the typical document
/// sizes we report (a few hundred MB max).
#[allow(
    clippy::cast_precision_loss,
    reason = "diagnostic-only display value; <1 MB rounding error is fine for a printf"
)]
fn bytes_mb(n: usize) -> f64 {
    n as f64 / 1_048_576.0
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let path = Path::new(&manifest)
        .join("../..")
        .join("samples")
        .join("bouten.afm");
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    println!(
        "loaded fixture: {} bytes from {} ({:.1} MB)",
        text.len(),
        path.display(),
        bytes_mb(text.len())
    );

    // Cold-start cost (DocState::new)
    let t = Instant::now();
    let state = DocState::new(text.clone());
    println!("DocState::new: {:?}", t.elapsed());

    let initial_spans = state.snapshot().gaiji_spans.len();
    println!("initial gaiji spans: {initial_spans}");

    // Edit at offset 0 — worst case for incremental TS reparse and
    // for changed_ranges (everything shifts).
    let t = Instant::now();
    state
        .apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())])
        .unwrap();
    println!("apply_changes (offset 0, cold path): {:?}", t.elapsed());
    let v1_spans = state.snapshot().gaiji_spans.len();
    println!("post-edit-1 gaiji spans: {v1_spans}");

    // Mid-document edit — the path designed for the incremental
    // algorithm. UTF-8-safe boundary at half the doc length.
    let snap = state.snapshot();
    let mid_target = snap.text.len() / 2;
    drop(snap);
    let mid_offset = nearest_char_boundary(&state, mid_target);

    let t = Instant::now();
    state
        .apply_changes(&[LocalTextEdit::new(mid_offset..mid_offset, " ".to_owned())])
        .unwrap();
    println!(
        "apply_changes (mid-doc @ {mid_offset}, incremental path): {:?}",
        t.elapsed()
    );
    let v2_spans = state.snapshot().gaiji_spans.len();
    println!("post-edit-2 gaiji spans: {v2_spans}");

    // Bench another mid-doc edit so caches stay warm and we have a
    // second data point.
    let t = Instant::now();
    state
        .apply_changes(&[LocalTextEdit::new(
            mid_offset + 1..mid_offset + 1,
            " ".to_owned(),
        )])
        .unwrap();
    println!(
        "apply_changes (mid-doc @ {} again, incremental path): {:?}",
        mid_offset + 1,
        t.elapsed()
    );

    // Synthetic gaiji-rich document so the incremental algorithm has
    // spans to carry forward. bouten.afm has zero gaiji (it's all
    // 傍点 annotations, a different node kind), which masks the
    // potential win — the cost reduction only manifests when there
    // is a non-empty old_spans set to skip re-extracting.
    println!("\n--- synthetic gaiji-rich doc ---");
    let gaiji_text = synth_gaiji_doc(50_000);
    println!(
        "synth doc: {} bytes ({:.1} MB)",
        gaiji_text.len(),
        bytes_mb(gaiji_text.len())
    );

    let t = Instant::now();
    let g_state = DocState::new(gaiji_text.clone());
    println!("  DocState::new: {:?}", t.elapsed());
    let g_initial = g_state.snapshot().gaiji_spans.len();
    println!("  initial gaiji spans: {g_initial}");

    // Cold path on synth doc
    let t = Instant::now();
    g_state
        .apply_changes(&[LocalTextEdit::new(0..0, " ".to_owned())])
        .unwrap();
    println!(
        "  apply_changes (offset 0, cold-degenerate): {:?}",
        t.elapsed()
    );

    // Mid-doc edit — incremental path should bypass walking 49 999
    // unchanged gaiji blocks.
    let snap = g_state.snapshot();
    let mid = nearest_char_boundary(&g_state, snap.text.len() / 2);
    drop(snap);

    let t = Instant::now();
    g_state
        .apply_changes(&[LocalTextEdit::new(mid..mid, " ".to_owned())])
        .unwrap();
    println!(
        "  apply_changes (mid-doc @ {mid}, incremental path): {:?}",
        t.elapsed()
    );

    let t = Instant::now();
    g_state
        .apply_changes(&[LocalTextEdit::new(mid + 1..mid + 1, " ".to_owned())])
        .unwrap();
    println!(
        "  apply_changes (mid-doc @ {} again, incremental path): {:?}",
        mid + 1,
        t.elapsed()
    );
    let g_final = g_state.snapshot().gaiji_spans.len();
    println!("  post-edits gaiji spans: {g_final}");
}

fn nearest_char_boundary(state: &DocState, target: usize) -> usize {
    let snap = state.snapshot();
    let mut idx = target.min(snap.text.len());
    while idx > 0 && !snap.text.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}
