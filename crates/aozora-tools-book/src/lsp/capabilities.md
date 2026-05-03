# Standard LSP capabilities

Every capability below is advertised in the `initialize` response and
works against any LSP-compliant client without additional opt-in.

## `textDocument/publishDiagnostics`

The server pushes diagnostics after each debounced semantic re-parse
(see [State model](state-model.md)). Each diagnostic carries:

- A code (e.g. `aozora::unclosed-bracket`).
- A severity (Error for paired-delimiter mismatches, Warning for PUA
  collisions and residual annotation markers).
- An optional `tags` array — `Unnecessary` is set for warnings the
  editor can grey out (PUA collisions).
- An opaque `data` payload that the `code_action` handler reads to
  build a quick-fix without re-parsing.

[Diagnostics catalogue](diagnostics.md) lists every variant.

## `textDocument/formatting`

Same code path as the `aozora-fmt` CLI (`aozora_fmt::format_source`).
The result is a single `TextEdit` covering the entire document with
the canonical form. Editors that show a "format on save" toast see
the formatter's idempotence guarantee directly: an already-canonical
document yields an empty edit list.

## `textDocument/hover`

Currently fires on **gaiji tokens** (`※［＃...］` and `※［＃「…」］`
forms). The hover content shows:

- The resolved Unicode codepoint (decimal + hex).
- The character itself.
- The mencode source (JIS X 0213 plane / row / cell).
- A note when the resolution went through `aozora_encoding::gaiji::resolve`'s
  fallback chain.

Future hover contributors (slug arguments, kaeriten markers) plug
into the same dispatcher in `aozora_lsp::hover`.

## `textDocument/completion`

Two completion sources:

1. **Slug catalogue** — every entry in `aozora::SLUGS`. Slugs that
   take arguments (e.g. `［＃「…」に傍点］`) come back as parametric
   snippets so the cursor lands inside the placeholder.
2. **Paired delimiter partner** — typing `「` while the cursor is
   not already inside a paired-delimiter context offers `」` as a
   "complete pair" item.

Trigger characters: `［`, `《`, `「`, `〔`, and `※`.

## `textDocument/linkedEditingRange`

When the cursor is inside `［...］`, `《...》`, `「...」`, `〔...〕`,
or any other paired delimiter the parser recognises, the open and
close bytes are linked as a single editing range. Editors that
support this LSP method (VS Code, Neovim with `vim.lsp`, Helix)
keep the pair balanced as the user types.

## `textDocument/foldingRange`

Folds at three granularities:

- Paragraph (`paragraph` kind in the LSP response).
- Container (`region` kind) — wraps `［＃ここから...］...［＃ここまで］`
  blocks.
- Heading section (`region` kind) — wraps everything from a
  heading-hint marker to the next heading-hint or container close.

## `textDocument/documentSymbol`

Returns a tree of symbols where each heading hint, page break, and
container open is a node. The hierarchy mirrors the parsed
`AozoraTree`'s container nesting, so editors render an outline that
matches the document's structural shape.

## `textDocument/semanticTokens`

Token categories: `kanjiBody`, `rubyBase`, `rubyReading`, `gaiji`,
`annotationKeyword`, `containerOpen`, `containerClose`, `pageBreak`,
`headingHint`, `kaeriten`. Modifiers: `unresolved` (gaiji whose
mencode could not be resolved), `paired` (delimiter that found its
partner).

The full token range is returned on `textDocument/semanticTokens/full`;
delta updates are advertised via `textDocument/semanticTokens/full/delta`
for editors that opt in.

## `workspace/executeCommand`

One command: `aozora.canonicalizeSlug`. See [Workspace commands](commands.md).
