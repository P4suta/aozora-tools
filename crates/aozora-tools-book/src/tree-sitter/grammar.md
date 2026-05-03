# Grammar surface

The grammar lives in
[`crates/tree-sitter-aozora/grammar.js`](https://github.com/P4suta/aozora-tools/blob/main/crates/tree-sitter-aozora/grammar.js).
This page is a navigational summary — for the full rule set, read
the file.

## Top-level production

```
document := paragraph (paragraph_break paragraph)*
```

Paragraph break is one or more blank lines; paragraph itself is a
sequence of inline tokens.

## Inline token kinds

| Token | Source surface | Notes |
|---|---|---|
| `kanji_body`     | run of CJK characters | The base for implicit ruby. |
| `kana_body`      | run of hiragana / katakana | |
| `latin_body`     | run of ASCII letters / digits | |
| `ruby`           | `｜<base>《<reading>》` or `<kanji_body>《<reading>》` (implicit) | The base/reading split is structural. |
| `double_ruby`    | `《《<text>》》` | Emphasis form. |
| `gaiji`          | `※［＃「説明」、第N水準A-B-C］` and `※［＃U+XXXX］` variants | The resolved character is not in the tree-sitter tree; resolution happens in the pipeline. |
| `bouten`         | `［＃「<base>」に傍点］` and bousen / 圏点 variants | |
| `tate_chu_yoko`  | `［＃「<digits>」は縦中横］` | |
| `kaeriten`       | `［＃<rank>］` (re-entry/return mark) | |
| `align_end`      | `［＃地付き］` and similar | |
| `indent`         | `［＃<n>字下げ］` | |
| `annotation`     | `［＃<keyword>］` whose keyword resolves through `aozora::SLUGS` | |
| `container_open` | `［＃ここから...］` family | |
| `container_close`| `［＃ここまで］` | |
| `page_break`     | `［＃改ページ］` and 改丁 / 改見開き variants | |
| `heading_hint`   | `［＃「<text>」は大見出し］` etc. | |
| `quote`          | `「...」` (paired) | |
| `tortoise`       | `〔...〕` | |
| `sashie`         | `［＃挿絵...］` | |

## What the grammar does not parse

- **Slug semantics** — `［＃改ページ］` and `［＃改丁］` both come
  out as `annotation` with the keyword embedded as a literal child;
  the slug catalogue lookup happens in the pipeline.
- **Gaiji resolution** — `gaiji` carries the mencode literally;
  `aozora_encoding::gaiji::resolve` produces the character.
- **Pair balance** — the grammar accepts unbalanced delimiters
  syntactically; the pipeline reports paired-bracket diagnostics.
- **Heading hierarchy** — `heading_hint` carries `大`, `中`, `小`
  literally; the document outline (level inference, nesting) is a
  pipeline concern.

## Edits the LSP relies on

`tree-sitter::Parser::parse` accepts a previous tree and an edit
descriptor. `aozora-lsp`'s `did_change` handler builds the edit
from the LSP `TextDocumentContentChangeEvent` array, applies it to
the rope buffer, then re-parses with the previous tree as input.
For an insertion in the middle of a 6 MB document, tree-sitter
re-parses only the affected region — typically tens of microseconds.

The incremental contract holds as long as the edit descriptor's
byte ranges are accurate. The position-encoding negotiation (UTF-8
vs UTF-16) at LSP `initialize` time exists specifically to keep
those byte ranges cheap: if the client picks UTF-8, no codepoint
conversion is needed.
