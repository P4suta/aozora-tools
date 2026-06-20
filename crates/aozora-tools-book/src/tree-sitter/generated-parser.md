# Generated parser.c

`crates/tree-sitter-aozora/src/parser.c` is committed to the
repository. Downstream consumers — the LSP server, the VS Code
extension, third-party tree-sitter hosts — only need a C toolchain
to build the crate.

## Why it is committed

`tree-sitter generate` produces `parser.c` from `grammar.js` via a
Node.js-based CLI. Committing the output spares every downstream
consumer from installing Node + `tree-sitter`, and is the convention
published tree-sitter grammars follow.

## When to regenerate

Regenerate after any change to `grammar.js`, `node-types.json`, or
the `external_scanner.c` (when present). The build runs the
upstream `tree-sitter` CLI, which is `mise`-managed:

```sh
mise install tree-sitter
cd crates/tree-sitter-aozora
tree-sitter generate
```

`tree-sitter generate` rewrites `src/parser.c` in place. Commit the
regenerated file as part of the same change that touched the
grammar source.

## Compiler warnings

Generated `parser.c` carries assorted clang warnings (unused function
arguments, signedness comparisons). `build.rs` silences them with
`-Wno-unused-parameter` / `-Wno-unused-but-set-variable` /
`-Wno-trigraphs`, so the C build stays clean without relaxing any Rust
lint. Hand-written code in `bindings/rust/` keeps the workspace defaults.

## Security review

Review the committed `parser.c` by reading the diff against the
previous version at the `tree-sitter generate` boundary. When a
scanner (CodeQL etc.) flags the generated source, adjust `grammar.js`
so the pattern no longer appears, regenerate, and commit both.
