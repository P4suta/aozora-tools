# ADR-0005 — `ArcSwap` snapshot for wait-free LSP reads

- Status: accepted (2026-04-27)
- Crate: `aozora-lsp`

## Context

Each open document holds a `DocState` inside an
`Arc<DashMap<Url, _>>`. Every LSP request handler — hover, inlay,
gaiji_spans, codeAction, completion, linked_editing — has to read
the current text + line-index + gaiji-span list. Until this ADR
landed, those reads went through a `dashmap` shard read lock that
the `did_change` writer held while applying a 200 ms tree-sitter
incremental reparse on a 6 MB document. The user-observed effect
was 1–2 seconds of inlay/diagnostic lag during keystroke bursts on
large docs (`samples/bouten.afm`, 6.3 MB).

Bench measurement:

```
apply_changes/insert_one_char_bouten_6mb     267 ms total
  └ ts_apply_edit_one_char                   202 ms
  └ gaiji_span_extract                        67 ms
  └ line_index_build                           3 ms
  └ apply_edits string copy                  0.3 ms
```

While the writer held the dashmap shard for ~270 ms, every
concurrent reader queued behind it. The architectural symptom: a
single-thread cost (TS reparse) becoming a multi-thread blocker.

## Decision

Split `DocState` into two halves:

- **`BufferState`** (writers only): `text`, `IncrementalDoc`,
  `SegmentCache`. Held behind one `parking_lot::Mutex`. Mutated on
  `apply_changes` / `replace_text` / debounced segment-cache
  reparse.
- **`Snapshot`** (readers only): `Arc<str> text`,
  `Arc<LineIndex>`, `Arc<BTreeMap<u32, Arc<GaijiSpan>>>`,
  `Option<Tree>`, `version`. The store is atomically swapped via
  `arc_swap::ArcSwap`; reads are wait-free (one atomic load + Arc
  clone).

`DocState::edit_version: AtomicU64` ratchets forward under the
buffer mutex on every applied edit. `Snapshot::version` records
the buffer state the snapshot was built from. A bg
`tokio::task::spawn_blocking` task observes the lag, reads
`(text, tree.clone(), version)` under a brief mutex acquisition,
computes line_index + gaiji_spans OFF the mutex, and
`ArcSwap::rcu`s the result in iff its version is at least as
fresh as the current snapshot. Older parallel rebuilds lose the
race silently.

Read path:

```rust
let state = self.lookup(&uri)?;       // dashmap shard read μs
let snap = state.snapshot();          // ArcSwap::load_full ~8 ns
let text = &snap.text;                // Arc<str>
let line_idx = &snap.line_index;      // Arc<LineIndex>
```

## Consequences

Positive:

- **Wait-free reads**, quantitatively proven by bench
  (`concurrent_reads/snapshot_load_*` in `benches/burst.rs`):
  - Solo (no writer): 8.16 ns
  - Under 267 ms write pressure: 8.07 ns
  - Same number to within noise — readers do not contend with
    writers.
- All LSP handlers became trivially correct against the read
  path: `state.snapshot()` returns an immutable `Arc<Snapshot>`
  whose contents are guaranteed valid for the lifetime of the
  Arc, regardless of subsequent edits.
- `did_change` returns in microseconds (the heavy snapshot
  rebuild is dispatched to the tokio blocking pool); subsequent
  LSP notifications are not held up by tower-lsp's notification
  ordering.
- The pattern set up natural follow-ups: `BTreeMap` keying allows
  the incremental gaiji rebuild (ADR-0003), the `Arc<str>` text
  field allows snapshot rebuilds to reuse text across snapshot
  generations.

Negative:

- Snapshot rebuild allocates a fresh `Arc<Snapshot>` per edit,
  plus an `Arc<str>` materialisation of the rope text. ~6 MB
  memcpy per snapshot on bouten.afm (~6 ms). Acceptable: the
  cost is OFF the writer hot path (in the bg blocking pool), and
  the snapshot lag is bounded to one rebuild.
- Snapshot can lag the buffer by one in-flight rebuild. Readers
  may see "the world as of one edit ago" briefly. Acceptable for
  hover/inlay/folding etc — the next snapshot install propagates
  within ~270 ms on the worst-case 6 MB doc.

## Alternatives considered

1. **Keep dashmap shard write lock; just shorten it** — reduce
   the apply_changes cost to <50 ms via per-component
   optimisation. Rejected: even at 50 ms hold time, 16 concurrent
   reads * 50 ms ≈ 800 ms tail latency. The architecture is
   wrong; only decoupling read/write paths fixes it.
2. **`RwLock<Snapshot>`** — readers acquire shared lock, writer
   acquires exclusive. Rejected: writers still serialise readers
   during the swap, so the worst-case under contention is still a
   spike. ArcSwap's RCU pattern is genuinely wait-free for
   readers.
3. **Per-paragraph snapshot** — finer-grained snapshot per chunk
   so the read lock is per-paragraph. Rejected as a stand-alone
   change: substantial complexity, AND ArcSwap already gives
   wait-free reads at coarser granularity. Per-paragraph segment
   work IS warranted but for the *parser* side (ADR follow-up,
   tracked in the SegmentedDoc commit, see #196).

## Verification

Bench: `cargo bench -p aozora-lsp --bench burst -- "concurrent_reads"
--warm-up-time 1 --measurement-time 3`

```
concurrent_reads/snapshot_load_solo               median  8.16 ns
concurrent_reads/snapshot_load_under_writer       median  8.07 ns
```

Tests: `cargo test -p aozora-lsp` — 159 lib tests pass, including
state.rs's `snapshot_loads_are_lock_free_after_install`,
`install_if_newer_rejects_stale_snapshots`,
`install_if_newer_accepts_equal_version_snapshots`.
