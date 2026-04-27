# ADR-0006 — `ropey::Rope` for the document buffer + tree-sitter chunked input

- Status: accepted (landed in `sxyxtyup perf(state, incremental)`, 2026-04-28)
- Crate: `aozora-lsp`

## Context

ADR-0001 (`ArcSwap` snapshot) decoupled the reader and writer
paths. The remaining writer-path cost on a per-edit basis was:

| Component                | Cost on bouten.afm (6 MB) |
|--------------------------|---------------------------|
| `text_edit::apply_edits` (String splice) | 0.3 ms          |
| `IncrementalDoc::apply_edit` (TS reparse) | 200 ms         |
| `LineIndex::new`         | 3 ms                      |
| `extract_gaiji_spans`    | 67 ms                     |

Two concerns drove this ADR:

1. **Editing scaling cliff**: `String` splice is `O(n)` — for a
   200 MB document (`samples/tsumi-to-batsu-x100.afm`) a single
   keystroke would cost ~10 ms just for the buffer mutation,
   wasted entirely on `memcpy`-ing unchanged bytes.
2. **TS reparse needs a contiguous `&str`**: every `apply_edit`
   handed tree-sitter a freshly-allocated `String` of the post-
   edit buffer. With a `String`-backed buffer this is unavoidable;
   with a Rope we can stream chunks and never materialise the
   whole text just for the parser.

Neither is dominant in the wall-time budget today (TS reparse at
200 ms swamps both), but both are *cliffs* the architecture
should not trip over once tree-sitter's cost is addressed
(ADR-0003 and the SegmentedDoc work).

## Decision

Replace `BufferState::text: String` with `ropey::Rope`. Add
`IncrementalDoc::parse_full_rope` and `apply_edit_rope` that feed
tree-sitter via `parser.parse_with_options` + a chunked-input
callback closing over the rope:

```rust
fn chunk_callback<'r>(rope: &'r Rope) -> impl FnMut(usize, Point) -> &'r [u8] {
    let len = rope.len_bytes();
    move |byte_idx, _pos| -> &'r [u8] {
        if byte_idx >= len { return &[]; }
        let (chunk, chunk_byte_idx, _, _) = rope.chunk_at_byte(byte_idx);
        let local = byte_idx - chunk_byte_idx;
        &chunk.as_bytes()[local..]
    }
}
```

`BufferState::apply_edits` validates ranges against the rope
(`rope_is_char_boundary` mirrors `str::is_char_boundary`
semantics) then splices in REVERSE source order so each edit's
pre-shift byte offsets stay valid against the still-pre-edit
prefix — no cumulative-delta math, no extra allocations.

`Snapshot::text` stays `Arc<str>`. The snapshot rebuild
materialises the rope into `Arc<str>` exactly once per snapshot
generation; LSP request handlers continue to take `&str` so the
rest of the LSP layer is untouched.

## Consequences

Positive:

- **`O(log n)` byte-range splice** via `Rope::insert` /
  `Rope::remove` (chunked B+ tree). The cliff at ~200 MB is gone.
- **TS chunked input**: tree-sitter walks the rope's internal ~1
  KB chunks directly. No 6 MB clone of the buffer per edit just
  to feed the parser.
- **Foundation for incremental snapshots**: `Rope::clone` is an
  Arc bump on the root node, so future work can keep multiple
  buffer versions cheaply (e.g. per-edit-version diff bases).
- All 159 lib tests pass — Rope edits go through the exact-same
  validation shape; cross-boundary / out-of-bounds / unsorted
  batches still reject with the existing `EditError` variants.

Negative:

- One `Rope::to_string()` per snapshot rebuild (the only
  contiguous-text materialisation that remains). ~6 ms on a 6 MB
  doc, paid OFF the writer hot path in the tokio blocking pool.
- `segment_cache` still takes `&str`, so the debounced semantic
  reparse pays one materialisation too. Refactoring the segment
  cache to consume rope chunks is deferred — the path is rare
  (≤ once per debounce window) and the cost is dominated by the
  parse itself, not the materialisation.

## Alternatives considered

1. **Stay with `String`** — bench showed string-copy is 0.3 ms,
   noise level. Rejected: the *cliff* is what matters, not the
   median. Once tree-sitter cost is tackled (ADR-0003,
   SegmentedDoc), `String`'s `O(n)` splice would be the new
   bottleneck on big docs.
2. **`xi-rope`** — an older, less-maintained Rope. Rejected:
   `ropey` is the canonical choice in the modern Rust editor
   space (used by helix, zed has its own variant, etc), has
   active maintenance, and ships a `chunk_at_byte` API that maps
   one-to-one onto tree-sitter's chunked-input contract.
3. **`crop`** — newer alternative to ropey, similar API,
   competing for "modern" status. Rejected: ropey is more
   established and we don't need crop's specific advantages
   (concurrent edits) for our single-writer model.
4. **Custom rope** — out of scope. ropey covers our requirements.

## Verification

Tests: 159 lib tests including `state.rs::install_if_newer_*` and
the `IncrementalDoc::*` round-trip tests cover the rope path.

Real-trace samply analysis (after this ADR landed): `ropey`
appears at 0.2 % of profile self-time on the bouten.afm bench —
i.e. the buffer-side cost is now noise relative to TS at 68 %.
