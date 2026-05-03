# Overview

`tree-sitter-aozora` is the tree-sitter grammar `aozora-lsp` uses
for incremental parsing inside the LSP server. It is also a
standalone artefact — any tree-sitter host (Neovim's
`nvim-treesitter`, Helix, Zed, Atom-style highlight engines) can
load the grammar and use it for syntax highlighting, fold ranges,
or structural search.

## Two parsers, one reason

The LSP server runs **two parsers per document**:

- **The aozora pipeline** (`aozora::Document::parse`) does the full
  semantic parse — paired-bracket validation, slug recognition,
  gaiji resolution, the works. It is whole-document and runs on the
  debounced re-parse path.
- **tree-sitter** does an incremental parse — every keystroke
  re-parses only the changed region. It produces a structural tree
  that is *good enough* for the synchronous parts of the LSP
  surface (linked editing, paragraph fold ranges, the per-keystroke
  `did_change` work).

Splitting this way gives `aozora-lsp` constant-time editor responsiveness
on a 6 MB document. The full pipeline running on every keystroke would
add tens of milliseconds of jitter at p99; the incremental tree-sitter
parse measures in tens of microseconds.

## When the tree-sitter parse alone is not enough

The tree-sitter grammar deliberately does not encode the full
semantic surface (slug catalogue lookups, gaiji resolution, container
nesting checks). It is a structural parser; the pipeline is a
semantic one. Hover, completion against the slug catalogue, gaiji
resolution, and diagnostics all run off the pipeline output, never
the tree-sitter tree.

## Outside the LSP

The grammar ships standalone for editors that want syntax
highlighting without running the LSP. The
[`bindings/rust/`](https://github.com/P4suta/aozora-tools/tree/main/crates/tree-sitter-aozora/bindings)
directory exposes a `LANGUAGE` constant you feed into a
`tree_sitter::Parser`:

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&tree_sitter_aozora::LANGUAGE.into())?;
let tree = parser.parse("｜青梅《おうめ》", None).unwrap();
```

Bindings for other host languages (Node, Python) are not published
from this repository; the upstream tree-sitter binding generators
work against `grammar.js` directly.
