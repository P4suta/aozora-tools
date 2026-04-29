# ADR-0007 — Incremental gaiji-span rebuild via `Tree::changed_ranges` + `Arc<GaijiSpan>` boxing

- Status: accepted (2026-04-28)
- Crate: `aozora-lsp`

## Context

`Snapshot::gaiji_spans` is the cache the inlay / aozora/gaijiSpans
handlers read on every LSP request — it must reflect the latest
buffer state. Until this ADR landed, every snapshot rebuild walked
the entire tree-sitter tree to re-extract every gaiji span:

```
extract_gaiji_spans on bouten.afm (6 MB, 0 gaiji)            67 ms (tree walk)
extract_gaiji_spans on synth doc (4.6 MB, 50 000 gaiji)     ~280 ms
```

That cost was paid on every keystroke for a 50 k-gaiji document.
Even with the wait-free reads of ADR-0001 (so reads don't *block*
on this), the snapshot lag — the time between an edit landing and
the snapshot reflecting it — was dominated by this re-walk plus
the per-span `String` allocations for `description` / `mencode`.

## Decision

Two complementary changes:

### Part 1: `Arc<GaijiSpan>` boxing with `Arc<str>` body fields

`Snapshot::gaiji_spans: Arc<BTreeMap<u32, GaijiSpan>>` becomes
`Arc<BTreeMap<u32, Arc<GaijiSpan>>>`, and the inner struct
switches description/mencode from `String` to `Arc<str>`:

```rust
pub struct GaijiSpan {
    pub start_byte: u32,
    pub end_byte: u32,
    pub description: Arc<str>,
    pub mencode: Option<Arc<str>>,
}
```

This makes carry-forward across snapshot generations cheap:

- **Unchanged byte offsets** → reuse the entire `Arc<GaijiSpan>`
  via `Arc::clone(span)` (single atomic increment).
- **Shifted byte offsets** → allocate a fresh `Arc<GaijiSpan>`
  with the new start/end, but pointer-bump the description /
  mencode `Arc<str>`s. Avoids the per-span `String` clone.

### Part 2: `Tree::changed_ranges`-driven incremental rebuild

`Snapshot::tree: Option<Tree>` is kept on the snapshot (cheap
shallow Arc clone of tree-sitter's internal). The next rebuild
calls `prev.tree.as_ref().changed_ranges(&new_tree)` to learn the
byte ranges where structure differs.

Algorithm (`extract_gaiji_spans_incremental`):

1. `old_tree.changed_ranges(&new_tree)` → byte ranges in NEW
   coordinates where structure differs. On worst-case edits
   (offset 0 insert) the iterator emits many small ranges; we
   sort + merge once into a non-overlapping list.
2. For every span in `old_spans`, apply the cumulative `edits`
   delta to translate `(start_byte, end_byte)` into NEW
   coordinates. Binary-search the merged ranges:
   - Span overlaps any changed range → drop (will be re-extracted
     in step 3).
   - Span outside every changed range → carry forward (with
     pointer-bump or shifted-Arc as in Part 1).
3. Single iterative `TreeCursor` walk that prunes against the
   merged range set — visits ONLY subtrees that intersect at
   least one changed range. Re-extracts gaiji nodes there.

Total cost: `O((n_spans + n_visits) * log n_ranges)`. For typical
edits (cursor in middle of doc) `n_visits` is small and `n_spans`
carry forward almost free.

`DocState::pending_edits: Mutex<Vec<(u64, InputEdit)>>` records
the edits since the last snapshot install. Drained on success;
preserved on RCU-loss so the next rebuild can still apply them.
`replace_text` clears the log (the cumulative-edit invariant
breaks at full replacement).

## Consequences

Positive:

- **Algorithmic correctness verified**: 5 test cases pin
  cold == incremental equivalence across edit shapes (isolated
  text, replaced description, new gaiji insert, offset-0 insert
  worst case, sole-gaiji deletion). On a 50 k-gaiji synth doc
  every span is preserved across edits (`post-edits gaiji spans:
  50000`).
- **Ready for the per-paragraph parser**: when the SegmentedDoc
  work brings TS reparse cost down from 200 ms to single-digit
  ms, the gaiji rebuild can become the dominant cost. The
  `BTreeMap` shape is what makes the per-segment pattern viable
  (range queries, partial reload).
- **Cheap carry-forward**: each shifted span costs ~100 ns
  (Arc<GaijiSpan> alloc + 2 Arc<str> bumps); each unshifted span
  costs ~10 ns. 50 k spans → ~3-5 ms total — vs the prior ~75-150
  ms of String allocations.

Negative:

- The `pending_edits` log adds coordination complexity:
  `apply_changes` pushes under the buffer mutex,
  `rebuild_snapshot_now` reads + retains-on-loss, successful
  install prunes by version. The invariant ("install
  succeeded ⟹ log can be pruned up to that version") is non-
  trivial; tested by the snapshot-rebuild paths in `state.rs`.
- The wall-time gain is **not currently measurable** in the
  bench because tree-sitter's incremental reparse (200 ms on
  bouten.afm) still dominates total per-edit cost. The
  architectural gain is correct and ready to pay off when the
  parser cost is reduced.

## Alternatives considered

1. **Full re-walk + skip dictionary path** — keep the simple
   `extract_gaiji_spans(tree, text)` and accept the 70 ms cost.
   Rejected: scales linearly with gaiji count, becomes the
   dominant cost once parser cost is addressed.
2. **`tree-sitter::Query` API** — declarative `(gaiji) @g`
   pattern. Tried; ran 5× slower than recursive walk
   (71 ms → 330 ms) because the QueryCursor's general pattern-
   matching automaton has more overhead per visited node than a
   single-kind dispatch. Documented in code comment +
   `gaiji_spans.rs` doc; we keep the iterative TreeCursor walk.
3. **Stash old gaiji-spans by content hash, dedup on rebuild** —
   reuse existing spans across versions by hashing
   `(description, mencode, byte_range)`. Rejected: the cost
   reduction is purely the per-span allocation (now solved by
   `Arc<str>`); the walk cost is unchanged.
4. **Per-segment span tables (SegmentedDoc)** — let the parser
   work be per-paragraph and have each segment maintain its own
   spans. ADR-0003-bis if/when SegmentedDoc fully wires through
   `BufferState`; the foundation crate `segmented_doc` is in
   place but not yet integrated.

## Verification

Tests: `cargo test -p aozora-lsp --lib gaiji` — 16 tests pass
including 5 `incremental_*` correctness pins.

Bench: `apply_changes/insert_one_char_*` — measured before and
after; snapshot still ~270 ms total because TS dominates, but the
gaiji portion's contribution is independently bench-able via
`subcomponents/gaiji_span_extract` (67 ms baseline = upper bound
on what an incremental rebuild can save on this fixture).

Real-trace samply analysis: `aozora_lsp` total time on the
bouten.afm bench is 8.5 % of profile self-time (the gaiji walk is
within that 8.5 % envelope; not separable without smaller bench
fixtures).
