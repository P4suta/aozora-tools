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

## 残タスク (次セッションへの引き継ぎ)

### 優先度 1: SegmentedDoc を BufferState に結線

`crates/aozora-lsp/src/segmented_doc.rs` は library として 10 tests pass で landed。
`BufferState::incremental: IncrementalDoc` を `SegmentedDoc` に置換して、`Snapshot::tree`
を `Vec<(Range<usize>, Tree)>` に変えると、per-edit のreparse cost が 220ms → ~3ms に。
影響範囲:
- `state.rs::BufferState`: `incremental` フィールド型変更
- `Snapshot`: `tree: Option<Tree>` → `segments: Arc<[Segment]>`
- `gaiji_spans.rs::extract_gaiji_spans*`: 現在 `&Tree` を取る → `&[Segment]` に変更、
  各 segment.tree を walk して segment.byte_range.start で offset 補正
- `incremental_gaiji_rebuild` ロジック: per-segment changed_ranges に対応 (より複雑)

### 優先度 2: tree-sitter grammar simplify

samply trace で `ts_subtree_summarize_children` (8.7%) と `ts_subtree_compress` (7.2%) が
hot。Grammar 内の冗長な choice / prec.dynamic を削れば parse table が縮む可能性。
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

## 物理状態

- jj working copy: 新 wip (この doc 用)
- 未 commit: `docs/handoff-2026-04-28-evening.md` のみ
- 実行中の background task: なし
- Tests: aozora-lsp 169 lib tests + 6 xtask + 26 aozora-encoding 全 pass
- Clippy: workspace clean
- Fmt: clean
- VS Code TS: `bun run check` clean
