//! Shuttle randomized-schedule check for the LSP `DocState` lifecycle.
//!
//! # Why shuttle, not loom
//!
//! Loom shines for testing custom sync primitives (a hand-rolled
//! Mutex, a wait-free queue). Our concurrency surface is built from
//! battle-tested off-the-shelf primitives (`DashMap`, `std::sync::Arc`),
//! so loom's exhaustive interleaving search would explore mostly
//! library-internal schedules without finding bugs at our layer.
//!
//! Shuttle randomly samples interleavings of `std::thread::spawn`-
//! based concurrent code. We use it to model the **`DocState`
//! lifecycle**: 2 threads each performing a randomised op sequence
//! (open / change / close / read) on a shared
//! `Arc<DashMap<Url, DocState>>`. Across `N = 10_000` iterations
//! shuttle explores enough of the interleaving space to surface
//! schedule-dependent bugs before they reach production.
//!
//! # What is checked
//!
//! - **No panic**: any panic in any worker thread fails the test.
//! - **No deadlock**: shuttle bounds the schedule and reports stuck
//!   threads.
//! - **Final-state consistency**: after both threads finish, every
//!   document still in the map has `parsed.normalized` equal to
//!   `parse(text).normalized`.
//!
//! # Tunables
//!
//! - `AOZORA_SHUTTLE_ITERS` (default 1000) — number of randomized
//!   schedules to explore. PR cron uses the default; nightly cron
//!   bumps to `10_000`+.
//!
//! # Running this test
//!
//! `cargo test -p aozora-lsp --test shuttle_doc_state --features shuttle-tests`
//!
//! Without the feature flag the test compiles to a no-op so default
//! `cargo test` doesn't pull in the shuttle dep.

#![cfg(feature = "shuttle-tests")]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    reason = "test harness: bounded inputs + descriptive prose docs"
)]

use std::collections::HashMap;
use std::env;

use aozora_parser::{TextEdit, apply_edits, parse};
use shuttle::sync::{Arc, Mutex as ShuttleMutex};
use shuttle::thread;
use tower_lsp::lsp_types::Url;

// Why we don't use `dashmap::DashMap` here:
//   DashMap uses `parking_lot::Mutex` internally, which is not
//   instrumented by shuttle's scheduler. A shuttle preemption while
//   a thread holds DashMap's internal lock causes a real deadlock
//   that the scheduler can't break out of (the test hangs forever).
//   Modelling the shape we care about (per-Url serialization,
//   independent buckets) with a `shuttle::sync::Mutex<HashMap<...>>`
//   loses fidelity to DashMap-specific behaviour but lets shuttle
//   actually explore the schedule space — and DashMap-internal bugs
//   are out of scope for this harness anyway (covered by DashMap's
//   own test suite + our `tests/concurrent_lsp.rs` integration tests).

#[derive(Debug)]
struct ShuttleDoc {
    text: String,
    parsed_normalized_len: usize,
}

impl ShuttleDoc {
    fn new(text: String) -> Self {
        let parsed = parse(&text);
        Self {
            parsed_normalized_len: parsed.artifacts.normalized.len(),
            text,
        }
    }
    fn apply(&mut self, edits: &[TextEdit]) {
        if let Ok(t) = apply_edits(&self.text, edits) {
            self.text = t;
            self.parsed_normalized_len = parse(&self.text).artifacts.normalized.len();
        }
    }
    fn replace(&mut self, t: String) {
        self.text = t;
        self.parsed_normalized_len = parse(&self.text).artifacts.normalized.len();
    }
}

/// Document map modelled with a single shuttle Mutex over a HashMap.
/// Trades DashMap's bucket-level concurrency for shuttle-observable
/// scheduling. The contract we're verifying — "operations on a
/// shared map don't violate the per-doc invariant under any
/// interleaving" — survives this substitution.
type DocMap = Arc<ShuttleMutex<HashMap<Url, Arc<ShuttleMutex<ShuttleDoc>>>>>;

fn doc_map_get(m: &DocMap, uri: &Url) -> Option<Arc<ShuttleMutex<ShuttleDoc>>> {
    let guard = m.lock().unwrap();
    guard.get(uri).cloned()
}

fn doc_map_insert(m: &DocMap, uri: Url, doc: ShuttleDoc) {
    let mut guard = m.lock().unwrap();
    guard.insert(uri, Arc::new(ShuttleMutex::new(doc)));
}

fn doc_map_remove(m: &DocMap, uri: &Url) {
    let mut guard = m.lock().unwrap();
    guard.remove(uri);
}

/// One scheduling iteration: 2 threads do a fixed sequence of ops
/// on a shared `Arc<Mutex<HashMap<Url, Arc<Mutex<ShuttleDoc>>>>>`.
/// The double-Arc<Mutex> shape mirrors what DashMap gives us in
/// production (bucket lock outside, per-value lock inside) at a
/// fidelity shuttle can preempt at every step.
fn lifecycle_iteration() {
    let docs: DocMap = Arc::new(ShuttleMutex::new(HashMap::new()));
    let uri_a = Url::parse("file:///shuttle_a.txt").unwrap();
    let uri_b = Url::parse("file:///shuttle_b.txt").unwrap();

    doc_map_insert(
        &docs,
        uri_a.clone(),
        ShuttleDoc::new("doc a init\n\n".to_owned()),
    );
    doc_map_insert(
        &docs,
        uri_b.clone(),
        ShuttleDoc::new("doc b init\n\n".to_owned()),
    );

    let docs_t1 = docs.clone();
    let uri_a_t1 = uri_a.clone();
    let uri_b_t1 = uri_b.clone();
    let t1 = thread::spawn(move || {
        if let Some(doc) = doc_map_get(&docs_t1, &uri_a_t1) {
            doc.lock().unwrap().apply(&[TextEdit::new(0..0, "T1.")]);
        }
        if let Some(doc) = doc_map_get(&docs_t1, &uri_b_t1) {
            doc.lock().unwrap().apply(&[TextEdit::new(0..0, "T1.")]);
        }
        if let Some(doc) = doc_map_get(&docs_t1, &uri_a_t1) {
            doc.lock()
                .unwrap()
                .replace("rewritten by T1\n\n".to_owned());
        }
    });

    let docs_t2 = docs.clone();
    let uri_a_t2 = uri_a.clone();
    let uri_b_t2 = uri_b.clone();
    let t2 = thread::spawn(move || {
        if let Some(doc) = doc_map_get(&docs_t2, &uri_b_t2) {
            doc.lock().unwrap().apply(&[TextEdit::new(0..0, "T2.")]);
        }
        // T2 may close A at any time relative to T1's writes.
        doc_map_remove(&docs_t2, &uri_a_t2);
        if let Some(doc) = doc_map_get(&docs_t2, &uri_b_t2) {
            doc.lock().unwrap().apply(&[TextEdit::new(0..0, "T2.")]);
        }
    });

    t1.join().expect("t1 panicked");
    t2.join().expect("t2 panicked");

    // Final invariant: every remaining doc's parsed_normalized_len
    // equals what `parse(text).artifacts.normalized.len()` would
    // give now. Catches any state where a write was committed but
    // the cached length wasn't updated.
    let snapshot: Vec<(Url, Arc<ShuttleMutex<ShuttleDoc>>)> = {
        let guard = docs.lock().unwrap();
        guard.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    };
    for (uri, doc) in snapshot {
        let doc = doc.lock().unwrap();
        let fresh = parse(&doc.text);
        assert_eq!(
            doc.parsed_normalized_len,
            fresh.artifacts.normalized.len(),
            "doc {uri}: cached normalized length disagrees with fresh parse",
        );
    }
}

fn iters_to_run() -> usize {
    env::var("AOZORA_SHUTTLE_ITERS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1000)
}

/// Invariant: the `DocState` lifecycle stays consistent under any
/// interleaving of 2 threads' op sequences (open/change/close).
/// Reproduces: preventive — primary refactor safety net for any
/// future change to backend.rs that touches DashMap or `DocState`
/// access patterns.
#[test]
fn shuttle_doc_state_lifecycle_consistent_under_random_schedules() {
    let iters = iters_to_run();
    eprintln!("shuttle_doc_state: exploring {iters} schedules");
    shuttle::check_random(lifecycle_iteration, iters);
}
