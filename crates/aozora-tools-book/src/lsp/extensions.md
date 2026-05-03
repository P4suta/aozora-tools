# Custom protocol extensions

Three surfaces are not part of the LSP base spec but are documented
here as stable wire contracts. Editor integrators can opt in by
sending the requests as JSON-RPC 2.0 over the same stdio channel
the LSP uses.

| Surface | Direction | Detail |
|---|---|---|
| [`aozora/renderHtml`](render-html.md) | client → server | Returns rendered HTML for the document at a given version. The VS Code extension drives the preview pane with this; other clients can do the same. |
| [`aozora/gaijiSpans`](gaiji-spans.md) | client → server | Returns every resolvable gaiji span in the document plus its resolved character. The VS Code extension drives inline-fold decorations with this; generic LSP clients can use the same data via `aozora_lsp::inlay_hints`. |
| [`aozora.canonicalizeSlug`](commands.md) | client → server (via `workspace/executeCommand`) | Rewrites the slug at the cursor to its canonical form. |

## Stability

Custom requests follow the same semver contract as the rest of the
library: a backwards-compatible field addition is a minor bump, a
removal or rename is a major bump. The JSON shapes in the linked
chapters are the wire contract and changes there will be flagged in
`CHANGELOG.md` under the **Custom LSP protocol** section.

## Discovery

The server does not advertise these requests in `initialize` because
the LSP spec has no `experimentalCapabilities` slot for arbitrary
custom requests with structured params. Clients send them
unconditionally; an editor that does not need them simply does not
ask. The server returns method-not-found-style errors for malformed
calls — never panics or drops the connection.

## Wire format conventions

Every custom request follows three conventions:

- **Method names use the `aozora/` prefix.** Reserves the namespace
  so future LSP capability additions cannot collide.
- **Params are object-shaped, not positional.** Tolerant to additive
  schema evolution.
- **Responses are object-shaped at the top level**, not raw arrays
  or scalars. Same reason — gives a forward-compatible slot for
  metadata.
