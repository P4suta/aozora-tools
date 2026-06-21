# Rendering to HTML

`aozora render` turns a document into HTML from the command line — the same
rendering the editor preview uses, without an editor.

```console
$ aozora render samples/ruby.afm
<p><ruby>日本<rp>(</rp><rt>にほん</rt><rp>)</rp></ruby></p>
```

## Input and output

`aozora render [PATH]` takes a single file, or `-` / no argument for stdin. By
default it writes the bare HTML **fragment** (byte-identical to the LSP's
[`aozora/renderHtml`](../lsp/render-html.md)) to stdout.

| Flag | Effect |
|---|---|
| `-o, --output <FILE>` | Write to `FILE` instead of stdout. |
| `--standalone` | Wrap the fragment in a self-contained HTML5 document with vertical-writing (`writing-mode: vertical-rl`) CSS for direct browser preview. |
| `--open` | Write a standalone document to a temp file and open it in the default browser (implies `--standalone`). |
| `--stats` | Print bytes-in / bytes-out / elapsed to stderr. |

## Examples

```sh
aozora render doc.afm > doc.html                  # fragment
aozora render --standalone -o preview.html doc.afm
aozora render --open doc.afm                      # preview in the browser
cat doc.afm | aozora render --standalone > preview.html
```

## Limits

Like the LSP, render applies a 16 MiB cap. Unlike the LSP — which returns an
inert notice to stay responsive on every keystroke — the one-shot CLI fails
loudly (exit `2`) so a `-o` pipeline never produces a useless artifact.
