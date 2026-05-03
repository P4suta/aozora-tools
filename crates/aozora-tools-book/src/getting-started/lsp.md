# LSP quickstart (any editor)

`aozora-lsp` speaks LSP over stdio. Any editor with a Language Server
Protocol client can use it.

## Neovim (built-in `vim.lsp`)

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = { "aozora" },
  callback = function(args)
    vim.lsp.start({
      name = "aozora-lsp",
      cmd = { "aozora-lsp" },
      root_dir = vim.fs.root(args.buf, { ".git" }),
    })
  end,
})
```

Pair with a `filetype.lua` entry mapping `*.afm`, `*.aozora`,
`*.aozora.txt` to the `aozora` filetype.

## Helix

`languages.toml`:

```toml
[[language]]
name      = "aozora"
scope     = "source.aozora"
file-types = ["afm", "aozora", "aozora.txt"]
roots     = [".git"]
language-servers = ["aozora-lsp"]

[language-server.aozora-lsp]
command = "aozora-lsp"
```

## Emacs (Eglot, built-in)

```elisp
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs
               '(aozora-mode . ("aozora-lsp"))))
```

## Zed

`~/.config/zed/settings.json`:

```jsonc
{
  "languages": {
    "Aozora": {
      "language_servers": ["aozora-lsp"]
    }
  }
}
```

## What you get

Once the server starts, your editor receives:

- **Diagnostics** — `［` ↔ `］` mismatches, `《` ↔ `》` mismatches,
  PUA collisions, residual `［＃...］` markers. Each diagnostic carries
  a structured payload that powers Quick-Fix actions in capable clients.
- **Formatting** — `textDocument/formatting` rewrites the buffer
  through the same code path as `aozora-fmt`.
- **Hover** — gaiji tokens (`※［＃...］`) reveal the resolved Unicode
  character with codepoint metadata.
- **Completion** — the `aozora::SLUGS` catalogue (every recognised
  keyword across the spec) drives auto-complete with parametric
  snippets where the keyword takes arguments.
- **Linked editing** — typing inside `［` or `《` highlights the
  paired close so renames stay balanced.
- **Folding ranges** — paragraph + container scopes.
- **Document symbols** — heading hints, page breaks, container starts.
- **Semantic tokens** — every token category (kanji body, ruby base,
  ruby reading, gaiji, annotation keyword, …) so themes can colour
  them independently.

The custom requests `aozora/renderHtml` and `aozora/gaijiSpans` give
clients access to the rendered HTML preview and per-gaiji
inline-decoration data; the VS Code extension uses them, but the wire
format is documented in [Custom protocol extensions](../lsp/extensions.md)
so any client can opt in.
