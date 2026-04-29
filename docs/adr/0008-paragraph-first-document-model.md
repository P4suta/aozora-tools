# ADR-0008 — Paragraph-first document model

- Status: accepted (2026-04-29)
- Crate: `aozora-lsp`
- Supersedes the whole-document `IncrementalDoc` of ADR-0006 for
  the writer-side state shape; ADR-0006's Rope + chunked-input
  technique is preserved per-paragraph.

## Context

ADR-0005 made reads wait-free by snapshotting writer state behind
`ArcSwap`. ADR-0006 swapped the writer's text storage to `Rope` so
edit application costs `O(log n)` instead of `O(n)`. ADR-0007 made
gaiji-span rebuilds incremental against the prior tree.

Together those landed wait-free reads, microsecond `did_change`
returns, and incremental gaiji extraction. But the writer's *parse*
path was still doing one whole-document tree-sitter reparse per
edit. We measured why:

```
subcomponents/ts_parse_full_60kb_slice            1.79 ms (30 ns/byte)
subcomponents/ts_parse_full_600kb_slice          19.5  ms (33 ns/byte)
subcomponents/ts_parse_full_bouten_6mb          215    ms (33 ns/byte)

subcomponents/ts_apply_edit_offset_0_bouten_6mb 220    ms
subcomponents/ts_apply_edit_mid_doc_bouten_6mb  217    ms
```

The aozora grammar produces a flat `repeat($._element)` at the top
level. Tree-sitter's incremental algorithm tries to reuse subtrees
above the edit, but that flat shape gives it almost nothing to
reuse — the parse cost is `~33 ns/byte` regardless of edit
position. On a 6 MB document each keystroke pays ~220 ms of
parser work, and ADR-0007's incremental gaiji rebuild is dwarfed
by that until the parser cost itself comes down.

The shape of the data tells us the answer. Aozora-flavored
markdown is almost always written as paragraphs separated by blank
lines; an edit affects exactly one paragraph 99 % of the time
(typing inside a paragraph is the loud-majority case; cross-blank
edits are typically `\n\n`-deletion or `\n\n`-insertion which
collapse to a small merge). So if we re-shape the writer's text
+ tree as a list of paragraphs, the per-edit parse cost drops to
`O(paragraph_size)` ≈ `O(1-10 KB)` ≈ `O(30 µs - 330 µs)`. The
gain comes from data structure choice, not from a smarter parser.

## Decision

The writer's source of truth is no longer "one Rope, one Tree". It
is a `Vec<MutParagraph>`, with each paragraph owning its own Rope
+ tree-sitter `Tree`. Reads see the corresponding
`Arc<[Arc<ParagraphSnapshot>]>` carrying immutable per-paragraph
text + line index + gaiji spans + tree.

### Module layout

```text
crate::paragraph
├── MutParagraph         (writers' single paragraph)
├── ParagraphSnapshot    (readers' single paragraph)
├── paragraph_byte_ranges(rope) → Vec<Range<usize>>   // split policy
├── build_paragraph_snapshot(p, byte_offset) → snap   // promote
├── ParagraphSnapshot::shifted_to(prior, new_start)   // Arc-style reuse
└── chunk_callback(rope) → impl FnMut...              // TS chunked input

crate::state
├── BufferState { paragraphs: Vec<MutParagraph>, parser, segment_cache }
└── Snapshot   { paragraphs: Arc<[Arc<ParagraphSnapshot>]>,
                 paragraph_starts: Arc<[u32]>,
                 total_bytes, version,
                 doc_text/doc_line_index/doc_gaiji_spans: OnceLock }
```

### Boundary policy

Split at every `\n\n` run. The first newline goes to the LEFT
paragraph; the second newline starts the RIGHT paragraph. Empty
rope yields one empty paragraph (`0..0`) so the rest of the code
can assume non-empty `paragraphs`. Hard cap at
`MAX_PARAGRAPH_BYTES = 64 KiB` so a never-blank-line input still
produces bounded segments.

This boundary preserves byte-for-byte equality with the source
when paragraphs are concatenated — the snapshot's lazy
`doc_text()` accessor depends on it.

### Edit flow

`apply_changes(edits)` validates the batch, then walks edits in
**reverse source order** (so each edit's pre-shift offsets stay
valid against the still-pre-edit prefix). For each edit:

1. `locate_byte(doc_byte) → (paragraph_idx, local_byte)` — `O(N)`
   walk over paragraph sizes (no cumulative-offset cache on the
   writer side; LSP keystroke rates with paragraph counts in the
   low hundreds keep this sub-microsecond).
2. If `start_para == end_para`: in-paragraph splice + chunked
   reparse of just that paragraph.
3. Else: zero-copy `Rope::append` of the prefix slice + new text
   + suffix slice → re-segment that merged content via
   `paragraph_byte_ranges` → splice the resulting `MutParagraph`s
   in for `paragraphs[start..=end]`.
4. `maybe_resegment_around(idx)` — if the affected paragraph grew
   past `MAX_PARAGRAPH_BYTES`, re-split it via the same range
   policy.

### Snapshot rebuild reuse

`build_snapshot(buffer, version, prior)` walks the new buffer's
paragraphs and, for each index, decides whether to reuse the prior
snapshot's `Arc<ParagraphSnapshot>`:

```rust
let snap = match prior.paragraphs.get(idx) {
    Some(prior_p) if prior_p.tree_id == live_id
                  && prior_p.byte_range.len() == paragraph.text.len_bytes() =>
    {
        ParagraphSnapshot::shifted_to(prior_p, new_start)
    }
    _ => Arc::new(build_paragraph_snapshot(paragraph, new_start)),
};
```

Three cases:

- **Same tree, same position** → `Arc::clone(prior_p)`. One atomic
  increment, zero allocations.
- **Same tree, shifted position** (a preceding paragraph grew or
  shrank) → allocate one fresh `Arc<ParagraphSnapshot>` whose
  `text` / `line_index` / `tree` fields are `Arc::clone`d from the
  prior; only the `byte_range` and `gaiji_spans` (with shifted
  offsets) are recomputed. ~100 ns per gaiji span shifted.
- **New tree (the edit hit this paragraph)** → full
  materialisation: rope → `Arc<str>`, build line index, walk tree
  for gaiji spans.

Cost summary: a single-paragraph edit on a doc with N paragraphs
costs `O(1 paragraph rebuilt + N - 1 Arc-bumps)`, dominated by
the one rebuilt paragraph's reparse + extract.

### Coordinate frames

`ParagraphSnapshot` deliberately mixes two coordinate frames:

- `byte_range`, `gaiji_spans[*].start_byte`, `gaiji_spans[*].end_byte`
  are **document-absolute**. Handlers that consume gaiji-spans
  don't need to know which paragraph a span comes from.
- `text`, `line_index`, `tree` are **paragraph-local**. The
  tree-sitter parse was fed only this paragraph's bytes; line/col
  numbers count this paragraph's `\n`s; the text starts at byte 0.

Mixing is intentional: gaiji spans being pre-shifted lets the
inlay / `aozora/gaijiSpans` handlers walk paragraphs without doing
any offset translation themselves; keeping `tree` paragraph-local
lets the Arc-shared tree pointer survive any number of position
shifts in preceding paragraphs.

### Lazy doc-level views

Many LSP handlers don't need the doc-flat `&str` at all — semantic
tokens, document symbols, folding ranges all walk paragraphs
directly. So the doc-wide views are lazy:

```rust
pub struct Snapshot {
    paragraphs: Arc<[Arc<ParagraphSnapshot>]>,
    ...
    doc_text: OnceLock<Arc<str>>,
    doc_line_index: OnceLock<Arc<LineIndex>>,
    doc_gaiji_spans: OnceLock<Arc<BTreeMap<u32, Arc<GaijiSpan>>>>,
}
```

Handlers call `snap.doc_text()` / `doc_line_index()` /
`doc_gaiji_spans()` if they need the flat view. The first call
materialises and caches the `Arc`; subsequent calls within this
snapshot's lifetime are free. Handlers that iterate paragraphs
directly never trigger materialisation at all.

## Consequences

Positive:

- **Per-edit cost drops in line with the data shape**. Measured on
  `bouten.afm` (6.3 MB, 36 009 paragraphs from `\n\n` splits, 0
  gaiji), the synchronous `apply_changes` end-to-end (validate +
  buffer mutate + paragraph reparse + snapshot rebuild) cost is:

  ```
  apply_changes/insert_one_char_bouten_6mb         152 ms  (was ~267 ms, -43%)
  apply_changes/insert_one_char_mid_doc_bouten_6mb 158 ms
  apply_changes/burst_100_inserts_bouten_6mb       5.43 s  (54 ms / edit, -83%)
  ```

  The wall is no longer dominated by tree-sitter (~220 ms) but by
  the snapshot rebuild walk over 36k paragraphs at ~4 µs each:
  Arc bumps, gaiji-span shift (0 spans on bouten so this is just
  the no-shift early-out), and the doc-byte sum. The single
  reparsed paragraph is now small (~183 bytes average) and costs
  microseconds. In production `apply_changes` returns in
  microseconds — the rebuild runs on the tokio blocking pool —
  this measurement is the worst-case wall on purpose.

- **Snapshot rebuild scales with the edit, not the doc**. A
  single-paragraph edit on a 1 000-paragraph doc rebuilds one
  paragraph and Arc-bumps 999 — total ~999 atomic increments + 1
  reparse, completing in ~30-300 µs depending on paragraph size.

- **Per-handler doc-view materialisation is opt-in**.
  `semantic_tokens_full` walks paragraphs directly, never
  materialising `doc_text`. `hover` / `inlay` materialise on first
  access but pay it once per snapshot generation (cached for
  every subsequent handler request against the same snapshot).

- **Cross-paragraph edits stay zero-copy on the prefix/suffix**.
  `apply_across_paragraphs` builds the merged Rope via
  `Rope::append(prefix_slice) + Rope::from(new_text) +
  append(suffix_slice)`; the prefix and suffix stay in
  ropey's structural-share territory, and only the (small) middle
  is owned-allocated.

- **Single source of truth for paragraph offsets**.
  `BufferState` holds only `paragraphs` (no cumulative-offset
  cache); `Snapshot::paragraph_starts` is rebuilt at snapshot
  time. We don't risk the two going out of sync because there's
  only one place either is computed.

Negative:

- **`locate_byte` is `O(N)` in paragraph count**. At LSP keystroke
  rates with the documents we measure (≤ 1 500 paragraphs) this is
  sub-microsecond, but pathological documents (a single 100 MB
  paragraph never broken by `\n\n`) would degrade to one giant
  paragraph parsed on every keystroke. The `MAX_PARAGRAPH_BYTES`
  cap is the safety net (forces a hard split at 64 KiB).

- **`apply_across_paragraphs` does a full reparse over the merged
  region**. We can't reuse either of the two old paragraphs'
  trees because their byte coordinates are stale; matching trees
  by something other than tree id would be a fragile shape-diff.
  For typical boundary-spanning edits (`\n\n` deletion or
  insertion) the merged region is bounded to ~10 KB; the reparse
  cost is acceptable.

- **The `tree_id == live_id && byte_len matches` reuse predicate
  is conservative**. If a paragraph reparses and produces a
  structurally identical tree (identical bytes too — e.g. an edit
  that net-no-ops) we'd still allocate a fresh `ParagraphSnapshot`
  because the tree id changed. Not worth a deeper structural
  equality check; the cost of an extra rebuild on the affected
  paragraph is bounded by paragraph size.

## Alternatives considered

1. **Whole-doc parse + smarter incremental algorithm**. We
   instrumented this path before reaching for segmentation: TS
   incremental reuse on this grammar saves single-digit %, not
   the order-of-magnitude we needed. The grammar shape itself is
   the limiter, not the algorithm. Rejected.

2. **Per-line segmentation**. Trivial split policy, but every
   line edit triggers a re-segment because lines almost always
   share nodes (ruby spans, brackets) with their neighbors. The
   `\n\n` boundary is the natural one because aozora grammar
   already treats blank lines as soft block separators.

3. **External "section" granularity** (chapter / heading-bounded).
   Coarser than paragraph; would have left chapter-internal edits
   paying chapter-size parse cost (still tens of milliseconds on
   long chapters). Paragraph is the smallest natural unit.

4. **Field-level incremental** — keep one Rope, and segment only
   the *parse* result (one Tree per paragraph, paragraphs computed
   on-the-fly from the Rope's `\n\n` index). Tried this in the
   `segmented_doc.rs` foundation crate before the rearchitecture
   proper. Worked, but the bookkeeping for mapping doc-absolute
   edits onto the segment list interleaved awkwardly with the Rope
   API. Going paragraph-first all the way down (writer + reader)
   removed an entire layer of translation code.

5. **Shape-diff tree reuse on cross-paragraph edits**. After
   `apply_across_paragraphs` resegments, we could try matching
   the new paragraphs against the old by `(byte_len, leading
   bytes hash)`. Rejected for now — the merged region is small,
   and the path is rare. If it becomes hot, ADR-0008-bis.

## Verification

Tests: `cargo test -p aozora-lsp --lib` — 161 lib tests pass,
including:

- `paragraph_byte_ranges_*` (4 tests) — split policy invariants:
  empty, single-paragraph, blank-line split, full coverage.
- `build_snapshot_extracts_local_gaiji_at_doc_absolute_offset` —
  per-paragraph build emits doc-absolute offsets correctly.
- `within_paragraph_edit_only_touches_one_paragraph_snapshot` —
  a one-character edit rebuilds exactly one paragraph snapshot;
  preceding + following paragraphs are `Arc::ptr_eq`-identical.
- `cross_paragraph_edit_resegments_correctly` — merge + resplit
  preserves byte content.
- `oversized_paragraph_resegments_to_max_bytes` — the cap kicks
  in.

Bench: `cargo bench -p aozora-lsp --bench burst -- "apply_changes"`

| Scenario                                     | Before     | After     | Δ      |
|----------------------------------------------|------------|-----------|--------|
| `insert_one_char_bouten_6mb` (offset 0)      | ~267 ms    | 152 ms    | -43 %  |
| `insert_one_char_mid_doc_bouten_6mb`         | ~217 ms    | 158 ms    | -27 %  |
| `burst_100_inserts_bouten_6mb`               | ~32 s      | 5.4 s     | -83 %  |
| `concurrent_reads/load_under_writer`         | 8.07 ns    | 8.04 ns   | noise  |

The remaining ~150 ms on bouten is the 36 009-paragraph snapshot
rebuild walk + paragraph_byte_ranges' single byte scan over the
6 MB rope. Both are linear in document size but O(constant) per
paragraph, where before they were O(constant) per *byte*. The
next obvious optimisation (if the wall ever shows up in a real
trace) is incremental `paragraph_starts` maintenance on the
writer side — but at LSP keystroke rates with the rebuild
on a blocking pool, the user-observed lag is dominated by
`did_change`'s microsecond-scale return, not this rebuild.

Snapshot rebuild reuse correctness: a one-character edit
rebuilds exactly one paragraph snapshot (asserted by
`within_paragraph_edit_only_touches_one_paragraph_snapshot`);
preceding paragraphs are `Arc::ptr_eq` identical, following
paragraphs share `text` / `line_index` / `tree` Arcs across the
shift. On gaiji-heavy fixtures every span is preserved across
edits because shifted paragraphs only re-shift their own gaiji
offsets, never re-walk the tree.

Lint: `cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo fmt --all -- --check` clean.
