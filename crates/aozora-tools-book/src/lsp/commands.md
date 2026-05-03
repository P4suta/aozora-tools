# Workspace commands

`aozora-lsp` exposes one workspace command, sent by the client via
`workspace/executeCommand`.

## `aozora.canonicalizeSlug`

Rewrites the annotation slug at the cursor position to the canonical
spelling registered in `aozora::SLUGS`.

### When to use it

The slug catalogue includes minor spelling variants the parser
accepts as equivalent (`改ぺーじ` ≡ `改ページ`, full-width vs
half-width digits in indent levels, …). The formatter rewrites
these on a whole-file pass; the `aozora.canonicalizeSlug` command
is the targeted equivalent for code-action / quick-fix flows where
a single slug needs to be canonicalised without reformatting the
rest of the document.

The VS Code extension wires this to a code action on every
non-canonical slug span, so the user gets a per-token quick-fix.
Other editors can register the command with their own UI hook
(`vim.lsp.buf.code_action()`, `M-x eglot-code-actions`, …).

### Request

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "workspace/executeCommand",
  "params": {
    "command": "aozora.canonicalizeSlug",
    "arguments": [
      {
        "uri": "file:///path/to/doc.aozora",
        "position": { "line": 12, "character": 8 }
      }
    ]
  }
}
```

### Response

`null` on success. The server applies the rewrite by sending a
`workspace/applyEdit` request back to the client; the client's
edit-application result determines whether the rewrite landed.

If the cursor is not inside a slug, the server returns `null`
without sending any edit — the command is a no-op rather than an
error. If the slug is already canonical, the same applies.

### Errors

| Code | When |
|---|---|
| `-32602` (Invalid params) | `arguments[0]` is missing required fields, or `uri` is not a known open document. |

### Example flow

1. Editor sees a `aozora::non-canonical-slug` diagnostic at line 12.
2. User invokes Quick Fix; client lists the
   `aozora.canonicalizeSlug` command from the diagnostic's `data`
   payload.
3. Client sends `workspace/executeCommand` with the diagnostic's
   range as the `position` argument.
4. Server computes the canonical replacement, sends
   `workspace/applyEdit` to the client, returns `null` to the
   command request.
5. Client applies the edit; the diagnostic is cleared on the next
   debounced re-parse.
