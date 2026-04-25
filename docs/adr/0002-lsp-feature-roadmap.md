# ADR-0002 — LSP feature roadmap

Status: accepted (2026-04)

## Context

The 0.2 `aozora` split delivered a clean editor-facing API surface
(`Document::parse → AozoraTree<'_>`, structured `Diagnostic`s,
`PairLink` side-table from Phase 1.1, slug catalogue + canonicaliser
from Phases 1.2/1.3, `node_at_source` from Phase 1.4). That surface
is what `aozora-tools` builds against. Without an explicit roadmap
the LSP layer drifts toward shipping whatever happens to be easy
next, instead of toward the editor experience the user explicitly
asked for.

## Decision

The LSP roadmap is split into three tiers and lands in the order
below. The four **must** entries are done; the rest are staged with
explicit rationale so a future contributor can pick up any one
without re-deriving the full menu.

### Tier 1 — must (shipped, Phase 2 of the sprint)

| # | LSP request | Source data | File |
|---|---|---|---|
| 1 | `textDocument/inlayHint` | `AozoraTree::source_nodes` → `Gaiji.ucs` (resolved Unicode scalar) | `inlay_hints.rs` |
| 2 | `textDocument/linkedEditingRange` | `AozoraTree::pairs()` (Phase 1.1) | `linked_editing.rs` |
| 3 | `textDocument/completion` | `aozora::SLUGS` (Phase 1.2) | `completion.rs` |
| 4 | `workspace/executeCommand aozora.canonicalizeSlug` | `aozora::canonicalise_slug` (Phase 1.3) | `commands.rs` |

### Tier 2 — nice (next sprint)

| # | LSP request | Source data | File (planned) |
|---|---|---|---|
| 5 | `textDocument/semanticTokens/{full,range}` | walk `AozoraTree.source_nodes` for Ruby / Bouten / Gaiji / Annotation / Container spans | `semantic_tokens.rs` |
| 6 | `textDocument/foldingRange` | container open/close spans + heading hints | `folding.rs` |
| 7 | `textDocument/documentSymbol` | `AozoraNode::AozoraHeading` + `HeadingHint` | `symbols.rs` |
| 8 | `textDocument/documentHighlight` | `AozoraTree::pairs()` | `highlight.rs` |
| 9 | `textDocument/codeAction` | per-`Diagnostic` quick-fixes (insert closing `]`, normalize slug variant, …) | `code_actions.rs` |

### Tier 3 — later

| # | LSP request | Source data | File (planned) |
|---|---|---|---|
| 10 | `textDocument/rename` | slug occurrences | `rename.rs` |
| 11 | `textDocument/selectionRange` | container nesting | `selection_range.rs` |
| 12 | `textDocument/codeLens` | per-chapter character / page count | `code_lens.rs` |

### Out of scope

- Tree-sitter grammar (separate ADR; the in-tree 4-phase lexer is
  faster and richer than tree-sitter could express).
- helix / neovim attach + objective tests (LSP is editor-agnostic
  in principle; per-editor smoke tests are a follow-on).

## Consequences

- The LSP advertises every Tier-1 capability in `initialize`.
- Each Tier-2/3 entry carries a planned file path so a contributor
  picking it up does not need to re-derive the architecture.
- `aozora`'s public surface has the four primitives Tier-1 needed —
  `pairs()`, `SLUGS`, `canonicalise_slug`, `node_at_source` /
  `source_nodes`. Tier 2/3 entries do not require new core surface
  beyond what the Phase 1 sprint already shipped.
