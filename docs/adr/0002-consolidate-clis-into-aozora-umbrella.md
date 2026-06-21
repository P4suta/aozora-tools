# 0002. Consolidate the CLIs into an `aozora` umbrella binary

- Status: accepted
- Date: 2026-06-21
- Deciders: aozora-tools contributors
- Tags: architecture, cli

## Context

The toolset shipped two binaries — `aozora-fmt` (formatter) and `aozora-lsp`
(language server). Two gaps followed:

- **Discoverability.** There was no single entry point; a newcomer had to know
  both binary names and what each did.
- **The diagnostic engine was editor-only.** The rich diagnostic catalogue
  (codes, severities, Japanese 何が起きた/どう直す messages, quick-fix payloads)
  lived inside `aozora-lsp` and could only be reached through an LSP client.
  CI pipelines, scripts, and non-VS-Code editor users had no command-line way to
  lint a document, render it to HTML, or read a diagnostic's explanation.

The project is pre-1.0 (v0.4.x), so a structural change to the CLI surface is
cheap to make now and expensive to defer.

## Decision

Introduce a single `aozora` binary (crate `aozora-cli`) with subcommands:
`fmt`, `lint` (alias `check`), `render`, `explain`, `lsp`, plus
`completions` / `man`. The diagnostic catalogue is extracted into a
renderer-agnostic `aozora-diagnostics` crate shared by the LSP (→ LSP
`Diagnostic`) and the CLI (→ terminal). `fmt` and `lsp` reuse the existing
crates (the latter via a new `aozora_lsp::serve()`), so there is one
implementation per surface.

The standalone `aozora-fmt` and `aozora-lsp` binaries are **kept unchanged** for
backward compatibility — notably, the VS Code extension launches `aozora-lsp`
by name.

## Consequences

- **Easier:** one discoverable command; terminal diagnostics for CI / scripts /
  any editor; HTML rendering and `explain` without an editor; one diagnostic
  source of truth shared by LSP and CLI.
- **Harder / cost:** an extra crate to publish (`aozora-cli`, last in order) and
  an extra one (`aozora-diagnostics`, first); `gen-assets` now produces a third
  binary's completions/man; the coverage ignore-regex gained
  `aozora-cli/src/main.rs`. The umbrella's `lsp`/`render --open`/`watch` entry
  points are inherently hard to unit-test (they serve forever, open a browser,
  or wait on the OS), so `aozora-cli`'s own coverage is lower than the
  library crates — the workspace aggregate stays above the 93 % gate.

## Alternatives considered

- **Re-route the VS Code extension through `aozora lsp`.** Rejected: the
  extension resolves `aozora-lsp` by name (bundled `server/aozora-lsp`, a
  `lsp.path` setting, a PATH fallback). Keeping the real binary is zero-risk;
  re-routing buys nothing and risks silent breakage.
- **Fold `lint` into `aozora-fmt`.** Rejected: it muddies the formatter's single
  responsibility (gofmt vs. go vet), and `aozora-fmt` would have to pull in the
  diagnostic terminal renderer.
- **Argv[0] multi-call dispatch (busybox-style).** Rejected: more fragile than
  shipping the real `aozora-fmt` / `aozora-lsp` binaries, with confusing failure
  modes for the editor and installers.

## References

- The implementation plan for this change.
- ADR [0001](./0001-record-architecture-decisions.md).
- `crates/aozora-diagnostics`, `crates/aozora-cli`.
