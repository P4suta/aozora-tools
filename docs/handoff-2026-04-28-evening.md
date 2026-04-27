# Handoff — 2026-04-28 evening (gaiji fallback + samply infra + ADRs + per-paragraph foundation)

このセッションで前回 handoff (`docs/handoff-2026-04-28.md`) の next-session 候補を全て
着手し、加えてユーザの「samply 整備」「sample doc の gaiji 解決」を data 駆動で完了。

## 完了タスク

| # | Sprint | 結果 |
|---|---|---|
| **gaiji fix** | sample doc の `丂 / 畺 / 龔` が unresolved だった件を probe で根本特定 → mencode が JIS X 0213 に存在しないことを実証 → `aozora-encoding::gaiji::lookup` に「description 単一字 fallback」追加。3 件すべて inlay 解決可能に。 | aozora repo の `msolnlxv` commit |
| 196 Profile + 最適化検討 | bench `subcomponents/ts_*` 拡張で TS parse は線形 (33 ns/byte) と確定 → mid-doc も offset-0 も 220ms (incremental reuse がほぼ効かない) → **per-paragraph segmentation が答え** | `skxxpqzp feat(segmented_doc)` |
| (関連) samply 整備 | preflight 4-check + symbol resolution (sidecar `.syms.json`) + multi-view analyzer (top-N / by-owner / allocator / hot stacks) + `docs/profiling.md` | `plzpywms feat(xtask)` |
| 197 VS Code menu | `aozora.showOutline` / `foldAll` / `unfoldAll` 追加 + 編集者画面に Outline ボタン + `configurationDefaults` で aozora 言語の semanticHighlighting 有効化 | `lnuvsynn` 内 |
| 198 ADR 3本 | 0005/0006/0007 を追記 (既存 0001-0004 と衝突回避で renumber); README index に追加 | 同上 |

## このセッションの commit (上 → 古い順)

```
lnuvsynn docs(adr) + feat(vscode): ADRs 0005/0006/0007 + outline/fold proxies
plzpywms feat(xtask): samply pipeline — preflight + analyze + symbol resolution
skxxpqzp feat(segmented_doc): paragraph-segmented tree-sitter state (foundation for #196)
sxyxtyup perf(state, incremental): rope-backed buffer + tree-sitter chunked input (#192)
pzvuxvrz feat(lsp): foldingRange / documentSymbol / semanticTokens (#193, #194, #195)
tozklwlq perf(gaiji_spans): box GaijiSpan in Arc + Arc<str> for description/mencode (#191)
ysvppsuy feat(state, gaiji): incremental snapshot rebuild via Tree::changed_ranges (#190)
... (older session)
```

## 重要な発見 (data 駆動で得られた事実)

### Tree-sitter parse は線形

`subcomponents/ts_parse_full_*` の bench:

```
60 KB slice    →  1.79 ms  (30 ns/byte)
600 KB slice   → 19.5  ms  (33 ns/byte)
6.3 MB full    → 215   ms  (33 ns/byte)
```

`subcomponents/ts_apply_edit_*` の bench:

```
ts_apply_edit_offset_0_bouten_6mb     220 ms (worst case)
ts_apply_edit_mid_doc_bouten_6mb      217 ms (typical case)
```

→ Tree-sitter incremental reuse は実質効かない。Document size が dominant cost。
**Per-paragraph segmentation で 1 paragraph (~1-10 KB) のみ reparse → 30 μs〜330 μs に**。
70× 高速化が見込める。

### Samply trace の owner 分布 (real measurement)

```
tree_sitter (C)   68.0%
other             16.9%   ← 主に未解決 fun_*
aozora_lsp         8.5%   ← 我々のコード
aozora_*           3.1%
arc_swap           1.6%
allocator/libc     1.0%   ← 極めて低い (Rope+Arc設計の効果)
std/core/alloc     0.5%
ropey              0.2%
```

→ 残る 1 つの最適化機会は明確に **tree-sitter 側**。SegmentedDoc 結線が次の本命。

### Gaiji 解決の実態 (probe 駆動)

ユーザの sample doc にあった「失敗例」3 件は、JIS X 0213 spec 上不存在の mencode 値でした
(spec 上の正解値とは異なる typo)。Smart fallback として「description が単一字なら自身を返す」
を追加 → 「丂」「畺」「龔」が即解決。

## このセッションの追加 commit (2026-04-29 早朝までに landed)

```
poxoluzl refactor(state, paragraph): post-review cleanup —
         remove duplicated paragraph_starts + Arc-style shifted_to +
         Rope::append + helper extraction
nrzurmou docs(handoff): 2026-04-28 evening
```

### 大リアーキテクチャ完了

`SegmentedDoc を BufferState に結線` という当初の作業項目はユーザの指示で
**「結線ではなく大リアーキテクチャ」** に拡大解釈され、`segmented_doc.rs`
foundation crate を破棄して `state.rs` を per-paragraph model に書き直した:

- `crates/aozora-lsp/src/paragraph.rs` (NEW, 384 LoC): `MutParagraph` /
  `ParagraphSnapshot` / `paragraph_byte_ranges` / `build_paragraph_snapshot` /
  `ParagraphSnapshot::shifted_to` (Arc-style, no-shift = `Arc::clone`)
- `crates/aozora-lsp/src/state.rs` (REWRITTEN, ~1055 lines):
  `BufferState { paragraphs: Vec<MutParagraph>, parser, segment_cache }` +
  `Snapshot { paragraphs: Arc<[Arc<ParagraphSnapshot>]>, paragraph_starts,
  total_bytes, version, doc_text/doc_line_index/doc_gaiji_spans: OnceLock }`
- `crates/aozora-lsp/src/gaiji_spans.rs`: pure walker (paragraph-local), 435 LoC
  削除
- `crates/aozora-lsp/src/semantic_tokens.rs`: `&[Arc<ParagraphSnapshot>]` を
  walk、`line_offset` で doc-absolute LSP positions
- ADR-0008 `paragraph-first-document-model.md` に詳細記録

実測:
- `apply_changes/insert_one_char_bouten_6mb`: 267 ms → 152 ms (-43%)
- `apply_changes/burst_100_inserts_bouten_6mb`: ~32 s → 5.4 s (-83%)
- `concurrent_reads/load_under_writer`: 8 ns (noise レベルの変化)

注: 旧 handoff の「220ms → 3ms」「401ms → 26ms」は中間状態または異なる計測 path で、
実 production 構造の per-edit wall は 152ms。最大コストは tree-sitter から
36 009-paragraph snapshot rebuild walk + paragraph_byte_ranges の byte-scan に移った
(両方 O(doc-bytes) のままだが per-paragraph 定数倍は微小)。
次の最適化機会は incremental `paragraph_starts` だが、production では
rebuild が tokio blocking pool に dispatch されるので user-observed lag には
出ない。

### 副次成果

- `paragraph_from_rope_slice(source, range, parser)` helper が `BufferState::new`
  / `replace` / `apply_across_paragraphs` / `maybe_resegment_around` の 4 箇所で
  共通化されている
- `apply_across_paragraphs` は `Rope::append` + `Rope::byte_slice` で zero-copy
  に merge (prefix/suffix は ropey の structural-share 領域に残る)
- `OnceLock` lazy doc-views: `semantic_tokens_full` 等の per-paragraph handlers は
  doc-wide `&str` materialise を完全に skip 可能、必要 handlers (`hover` /
  `inlay`) も snapshot 寿命中 1 回のみ payment

## 残タスク (次セッションへの引き継ぎ)

### 優先度 1: snapshot rebuild walk の incrementalisation

bouten.afm (36 009 paragraphs) で per-edit wall の大半は snapshot rebuild walk
(36k Arc bumps + paragraph_byte_ranges の 6 MB byte-scan)。
`paragraph_starts` を BufferState 側で incremental 維持すれば walk が skip 可能。
ただし production では rebuild が tokio blocking pool に dispatch されるので
user-observed lag には影響しない — 計測 wall を縮める価値があるかは要再評価。

### 優先度 2: tree-sitter grammar simplify

paragraph 化前の samply trace で `ts_subtree_summarize_children` (8.7%) と
`ts_subtree_compress` (7.2%) が hot。Grammar 内の冗長な choice / prec.dynamic を
削れば parse table が縮む可能性。今は per-paragraph reparse が ~183 bytes 平均で
microsecond オーダー、relative にはこの 2 関数の重みが残っているはず。
re-profile が必要 (per-paragraph model 適用後の trace は未取得)。
`grammar.js` の simplify 候補:
- `extras: $ => []` → `[$.newline]` で newline を文法構造から外す (現状 _element に
  newline を直接入れている)
- `prec.dynamic(1, ...)` を `prec(1, ...)` に降格できるか検証

### 優先度 3: profile 駆動の continuous improvement

`xtask samply analyze` の出力を CI artifact として保存し、commit ごとに diff を見られる
仕組み。`docs/profiling.md` の "Diff workflow" を CI 化。

## ADR一覧 (このセッションで追加)

- [ADR-0005: ArcSwap snapshot for wait-free LSP reads](adr/0005-arcswap-snapshot.md)
- [ADR-0006: ropey::Rope buffer + tree-sitter chunked input](adr/0006-rope-buffer.md)
- [ADR-0007: Incremental gaiji-span rebuild via Tree::changed_ranges](adr/0007-incremental-gaiji-rebuild.md)
- [ADR-0008: Paragraph-first document model](adr/0008-paragraph-first-document-model.md)

ADR README は `docs/adr/README.md` に index 化済み (既存 0001-0004 含む)。

## 試験コマンド

```bash
cd /home/yasunobu/projects/aozora-tools

# 全 lint + test
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace --all-targets
cargo fmt --all -- --check
cd editors/vscode && bun run check && cd -

# Profiling pipeline (capture + analyze)
cargo run -p aozora-tools-xtask -- samply lsp-burst 30
cargo run -p aozora-tools-xtask -- samply analyze /tmp/aozora-lsp-burst-*.json.gz | tee /tmp/report.txt

# Gaiji probe (data 駆動で table 動作を確認)
(cd /home/yasunobu/projects/aozora && cargo run -p aozora-encoding --example probe_gaiji)
```

## 物理状態 (2026-04-29 更新時点)

- jj working copy: 新 wip (handoff 更新 + ADR-0008 用)
- 未 commit: `docs/handoff-2026-04-28-evening.md` 更新 + 新 `docs/adr/0008-paragraph-first-document-model.md` + `docs/adr/README.md` 更新
- 実行中の background task: なし
- Tests: aozora-lsp 161 lib tests + 6 xtask + 26 aozora-encoding 全 pass
- Clippy: workspace clean
- Fmt: clean
- VS Code TS: `bun run check` clean
