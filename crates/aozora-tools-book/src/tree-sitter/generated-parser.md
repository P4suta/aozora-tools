# Generated parser.c

`crates/tree-sitter-aozora/src/parser.c` is committed to the
repository. Downstream consumers — the LSP server, the VS Code
extension, third-party tree-sitter hosts — only need a C toolchain
to build the crate.

## Why it is committed

Tree-sitter generates `parser.c` from `grammar.js` via the
`tree-sitter` CLI, which itself depends on Node.js. Asking every
downstream consumer to install Node + `tree-sitter` to regenerate
a file the upstream maintainer can vouch for is the wrong friction
trade. A committed `parser.c` is the convention every published
tree-sitter grammar follows for the same reason.

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

## Lint suppression

Generated `parser.c` carries assorted clang warnings (unused
function arguments, signedness comparisons) that the workspace's
`-D warnings` policy would otherwise reject. The crate's
`Cargo.toml` opts the `unused` lint group to `allow` *for this
crate only*:

```toml
[lints.rust]
unused = "allow"
```

Hand-written code in `bindings/rust/` keeps the workspace defaults.
The narrow allow is the recommended pattern for vendored
machine-generated code; widening it to other crates would defeat
the bug-detector value of the workspace lint policy.

## Security review

The committed `parser.c` is reviewed at the `tree-sitter generate`
boundary by reading the diff against the previous version. When
CodeQL or another scanner flags an alert in the generated source,
the recommended workflow is to adjust `grammar.js` so the offending
pattern no longer appears, regenerate `parser.c`, and commit the
two together.
