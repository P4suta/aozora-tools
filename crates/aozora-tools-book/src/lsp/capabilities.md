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

Three completion sources merge into one response (the client's own
ranker decides ordering):

1. **Slug catalogue** — every entry in `aozora::SLUGS`, fired when the
   cursor sits in a slug-open context (`［＃`, `[#`, `［#`, or `[＃`).
   Half-width openers are rewritten to the canonical `［＃…］` form on
   accept, and paired slugs append their close marker as an additional
   edit.
2. **Half-width emmet** — typing `[`, `]`, `<`, `>`, `|`, or `*` offers
   the matching full-width glyph (`［`, `］`, `《…》`, `》`, `｜`, `※`).
   This is the popup fallback; the primary surface is `onTypeFormatting`
   (below), which converts on every keystroke without a popup.
3. **Structured snippets** — snippet items with `${…}` tab-stops that
   expand into a fully-structured form (e.g. `［＃改ページ］`) and leave
   the cursor on the next placeholder.

Trigger characters: `＃`, `#`, `「`, `[`, `]`, `<`, `>`, `|`, `*`, `｜`,
`《`, and `※`.

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

Three token types, emitted as the standard LSP types and mapped from the
tree-sitter parse: `macro` (gaiji, `※［＃…］`), `enum` (the base 漢字 of
a ruby pair), and `string` (the reading inside `《…》`). No token
modifiers are published.

Only `textDocument/semanticTokens/full` is advertised — neither range
requests (`range: false`) nor `full/delta` updates are offered.

## `workspace/executeCommand`

One command: `aozora.canonicalizeSlug`. See [Workspace commands](commands.md).
