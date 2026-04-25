# ADR-0003 — Position encoding (UTF-16 column ↔ byte)

Status: accepted (2026-04)

## Context

LSP `Position` carries `(line, character)` where `character` is a
UTF-16 code unit offset by default (LSP spec). The aozora parser
exclusively works in UTF-8 byte offsets (`Span { u32, u32 }`). Every
LSP request and response that touches a `Position` therefore needs
a translation step.

## Decision

`crates/aozora-lsp/src/position.rs` carries the canonical pair:

- `position_to_byte_offset(source: &str, p: Position) -> Option<usize>`
- `byte_offset_to_position(source: &str, byte_offset: usize) -> Position`

Both functions:

- Treat `\n` as the only line terminator (the parser's Phase 0
  sanitization collapses `\r\n` and `\r` to `\n`, so the buffer LSP
  receives matches what the parser sees).
- Implement UTF-16 ↔ UTF-8 translation by walking each line's chars,
  accumulating UTF-16 code units against the LSP `character` count.
- Return `None` (resp. clamp to line end) for positions past the
  document end, matching the lenient behaviour LSP clients expect.

### LSP 3.17 `positionEncoding` capability

Modern clients (VS Code 1.84+, neovim 0.10+ builtin LSP) advertise
the `positionEncoding` capability with `["utf-8", "utf-16"]`. When
the negotiation lands on `utf-8` the translation step collapses to
identity — the parser's byte offsets pass through unchanged. The
current implementation does not yet advertise the capability; the
Tier-2 work pulling this in is a 30-line patch:

1. Read `client_capabilities.general.position_encodings` in
   `initialize`.
2. Pick `utf-8` when present, fall back to UTF-16 otherwise.
3. Set `server_capabilities.position_encoding` accordingly.
4. Branch on the chosen encoding in `position.rs` (identity vs the
   current UTF-16 walk).

This ADR pins the design decision so future work can land
`positionEncoding` without re-deriving the rationale.

## Consequences

- Every LSP request handler inside `aozora-lsp` calls
  `position_to_byte_offset` / `byte_offset_to_position` rather than
  doing inline arithmetic.
- The diagnostic mapping in `diagnostics.rs` translates each
  `Diagnostic::span` (a `Span { start, end }` of source bytes) to an
  LSP `Range` via these helpers.
- `inlay_hints.rs` and `linked_editing.rs` use the same helpers —
  any future `positionEncoding=utf-8` toggle benefits all callers
  via a single switch.
