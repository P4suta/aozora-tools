# ADR-0001: Shuttle randomized-schedule check for the LSP DocState
lifecycle

## Status

Accepted (2026-04-25).

## Context

`aozora-lsp` keeps per-document state in
`Arc<DashMap<Url, DocState>>`. Multiple async tasks (one per
inflight LSP request) can call `did_open` / `did_change` /
`did_close` / `hover` in parallel. The DashMap bucket lock
serialises mutations to the same Url; mutations to different Urls
proceed independently.

This works under the assumptions:

1. DashMap's per-bucket lock genuinely serialises `get_mut` for
   the same key.
2. Reads (`get`) see committed state, never half-written
   intermediate state.
3. Operations on different keys don't observably interfere.
4. Async cancellation between awaits leaves the synchronous
   mutation block whole (we don't hold the DashMap entry borrow
   across an await point).

Each is documented and we believe it. Property tests cover the
input-shape side; the sequential `tests/concurrency_regressions.rs`
covers known patterns. What's missing is a check that random
*scheduling* of these operations doesn't break the invariants —
particularly across future refactors that might add an `await` in
the wrong place or share a lock-free data structure.

The companion ADR `aozora-parser/docs/adr/0007-concurrency-test-strategy.md`
records why we chose Shuttle over Loom and which other tools we
explicitly skipped. This ADR documents what specifically Shuttle
verifies and the boundaries of that verification.

## Decision

### What Shuttle verifies

`tests/shuttle_doc_state.rs` defines a *lifecycle iteration*: two
threads, each performing a fixed sequence of operations
(open / change / close) on a shared
`Arc<DashMap<Url, ShuttleMutex<ShuttleDoc>>>`. Shuttle samples N
randomised interleavings of those threads' instructions
(`AOZORA_SHUTTLE_ITERS`, default 1000, nightly cron 10 000+).

Per iteration we assert:

- **No panic**: any worker panic fails the test with the schedule
  reproducer.
- **No deadlock**: Shuttle bounds the schedule and reports stuck
  threads.
- **Final-state consistency**: every document still in the map
  has its cached `parsed_normalized_len` equal to
  `parse(text).artifacts.normalized.len()`. A divergence indicates
  a write committed but the cached field wasn't updated, which is
  the canonical "stale cache" bug shape.

### What Shuttle does NOT verify

- **Tokio runtime semantics**: Shuttle uses `std::thread`. The
  real LSP backend uses tokio tasks; cancellation, await points,
  and runtime scheduling are not modeled. The
  `regression_async_cancellation_leaves_consistent_state` test in
  `tests/concurrency_regressions.rs` covers tokio cancellation
  separately.
- **DashMap internals**: we treat DashMap as a black-box thread-safe
  map. Bugs inside DashMap itself are out of scope (the maintainers
  test them).
- **DashMap concrete behaviour under shuttle**: shuttle's scheduler
  preempts at every `std::sync` operation but cannot preempt
  `parking_lot::Mutex` (which DashMap uses internally). A naive
  attempt to use a real `Arc<DashMap<...>>` inside the shuttle
  iteration deadlocks on the first preemption. We therefore
  substitute `Arc<shuttle::sync::Mutex<HashMap<Url,
  Arc<shuttle::sync::Mutex<ShuttleDoc>>>>>` for the map. This is
  a *fidelity downgrade* — bucket-level concurrency is collapsed —
  but the contract being verified ("operations on a shared map
  don't violate the per-doc invariant under any interleaving") is
  preserved. DashMap-specific bugs are out of scope for this
  harness; the integration tests in `tests/concurrent_lsp.rs`
  exercise the real DashMap.
- **Long-tail rare schedules**: 10 000 iterations is a large sample
  but not exhaustive. Loom would be — at the cost of intractable
  runtime for our workload (see ADR-0007).

### Why Shuttle, not Loom

(Detailed in ADR-0007.) Short version: we don't write custom sync
primitives, so Loom's exhaustive schedule exploration would mostly
explore library internals. Shuttle's randomized sampling at higher
abstraction level is the better fit.

## Consequences

### Positive

- Future refactors that touch DashMap access patterns or DocState
  field initialisation get checked against thousands of schedules
  before merging.
- The test name + invariant doc comment serves as a contract
  reminder: "the cached length must match the actual parse".
- Gated behind `--features shuttle-tests` so default builds skip
  the shuttle dependency entirely.

### Negative

- The lifecycle iteration is hand-coded with two threads and a
  fixed op sequence per thread. Adding new ops (e.g. a future
  `did_save` handler) requires updating the harness.
- Shuttle's ~7 MB of dependencies pull in when the feature is
  enabled. Acceptable on nightly cron, undesirable on every PR.

## Future expansions

If we add:

- a new sync primitive (e.g. an `Arc<RwLock<SegmentCache>>` to
  share a cache across documents), expand the harness to model it
  and consider promoting to Loom for that primitive.
- async cancellation across multiple awaits, port the harness to
  `shuttle::future` and the tokio-compatible scheduling.

## Verification

```sh
# Default short run
cargo test -p aozora-lsp --features shuttle-tests --test shuttle_doc_state

# Nightly cron
AOZORA_SHUTTLE_ITERS=10000 cargo test -p aozora-lsp \
    --features shuttle-tests --test shuttle_doc_state
```
