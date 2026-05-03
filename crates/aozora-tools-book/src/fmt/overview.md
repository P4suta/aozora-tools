# Overview

`aozora-fmt` is a thin CLI on top of one library function:

```rust
pub fn format_source(input: &str) -> String;
```

The function parses `input` with `aozora::Document::parse`, walks the
returned `AozoraTree`, and serialises it back to text. The same code
path runs inside `aozora-lsp` when an editor sends
`textDocument/formatting`, so the LSP and the CLI are guaranteed to
agree on the canonical form of any document.

## Idempotence

`format_source(format_source(x)) == format_source(x)` for every input.
The formatter has no "second-pass" rewrites: every transformation is
expressed as a difference between the parsed `AozoraTree` and its
serialised form. If the tree round-trips to the input byte-for-byte,
no rewrite is emitted; otherwise the serialisation is the rewrite.

## What the formatter changes

The formatter is **conservative** — it changes only what the parser
recognises as semantically distinct from the canonical form:

- **Implicit ruby → explicit** — `日本《にほん》` becomes
  `｜日本《にほん》`. The implicit form is a parser convenience for
  human authors; the canonical form is unambiguous about the
  ruby-base extent.
- **Whitespace normalisation inside annotations** — `［＃ 改ページ ］`
  becomes `［＃改ページ］`. The annotation grammar treats interior
  whitespace as insignificant; collapsing it removes a degree of
  freedom that diff tools would otherwise flag.
- **Container delimiter alignment** — block-container open/close
  markers (`［＃ここから...］` / `［＃ここまで］`) are pinned to
  column 0 of their own line so visual nesting matches structural
  nesting.

## What the formatter does not change

- **Line endings** — preserved as authored (LF, CRLF, mixed).
  The tree carries a `line_terminator` hint per paragraph that the
  serialiser respects.
- **Encoding** — UTF-8 in, UTF-8 out. SJIS round-tripping is the
  responsibility of the surrounding pipeline (`aozora-encoding`),
  not the formatter.
- **Comment-like trailing whitespace** — Aozora notation has no
  comment syntax; trailing spaces inside body text are preserved
  because the parser cannot distinguish "intentional" from
  "accidental".
- **Order of independent annotations on the same character** —
  `［＃「青」に傍点］［＃「青」に傍線］` and the same pair in the
  reverse order are semantically equivalent; the formatter does not
  re-order them.

## When the formatter cannot run

If `Document::parse` produces *any* `Diagnostic::UnclosedBracket` or
`Diagnostic::UnmatchedClose`, the formatter still runs — the parser
does paired-bracket recovery and the recovered tree round-trips
cleanly — but the rewrite may shuffle text around the offending
delimiter. `aozora-fmt --check` returns `1` in this case, which is
the same signal "needs rewrite" mode emits for normalising edits.
Editors that read diagnostics out of band should suppress automatic
format-on-save while a `UnclosedBracket` / `UnmatchedClose` diagnostic
is live.

The CLI never panics on malformed input. Unrecoverable I/O errors
(missing file, permission denied) exit `2` with a one-line message
on stderr.
