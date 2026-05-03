# Diagnostics catalogue

Every diagnostic the parser emits is mapped to an LSP `Diagnostic`
in `aozora_lsp::diagnostics`. Each variant has:

- A stable `code` (`aozora::*` namespace).
- A severity routing rule (Error / Warning).
- An optional `tags` set (`Unnecessary`).
- An opaque `data` payload that the `code_action` handler reads to
  build a quick-fix without re-parsing.

The catalogue below mirrors the dispatcher in
[`crates/aozora-lsp/src/diagnostics.rs`](https://github.com/P4suta/aozora-tools/blob/main/crates/aozora-lsp/src/diagnostics.rs).

## `aozora::source-contains-pua`

**Severity** — Warning. **Tag** — `Unnecessary`.

A private-use codepoint (`U+E001..U+E004` reserved by the lexer's
sentinel scheme) is present in the source. The parser cannot tell
this apart from a marker it inserts itself, so the surrounding text
becomes ambiguous.

**Quick fix** — Delete the offending codepoint. The diagnostic's
`data` payload carries the codepoint so the action can be built
without re-classifying the span.

## `aozora::unclosed-bracket`

**Severity** — Error.

An open delimiter (`［`, `《`, `「`, `〔`) reached end-of-input or
end-of-paragraph with no matching close on the pairing stack.

**Quick fix** — Insert the matching close at the diagnostic's
range end. The `data` payload carries the `pair_kind` and the
expected close character.

## `aozora::unmatched-close`

**Severity** — Error.

A close delimiter (`］`, `》`, `」`, `〕`) appeared with an empty
pairing stack, or with a stack top of a different `PairKind`.

**Quick fix** — Delete the unmatched close. The verbose message
also lists three other manual recovery options (insert the missing
open earlier, fix a mis-matched intermediate close, etc.) for cases
where the deletion is wrong.

## `aozora::residual-annotation-marker`

**Severity** — Warning.

A `［＃...］` annotation pair survived classification — the keyword
inside did not match any entry in the slug catalogue. Most often
this is a typo (`改ぺじ` for `改ページ`) or an unsupported keyword.

**Quick fix** — None. The user must choose what they meant; the
verbose message lists the recovery checklist (slug name typo,
missing mencode, fall back to description-only `※［＃「説明」］`).

## `aozora::unregistered-sentinel`

**Severity** — Error.

A pipeline-internal sanity check failed — the lexer emitted a
sentinel codepoint at a position the placeholder registry has no
entry for. This is a bug in `aozora-pipeline`, not in user input.

**Quick fix** — None; the message asks the user to file a bug
with the offending source.

## `aozora::registry-out-of-order`

**Severity** — Error. Same class as `unregistered-sentinel` — a
pipeline-internal sanity check.

## `aozora::registry-position-mismatch`

**Severity** — Error. Same class as above.

## `aozora::unknown-diagnostic`

**Severity** — Warning.

`aozora::Diagnostic` is `#[non_exhaustive]`. A future parser
release may emit a variant the current `aozora-lsp` does not
recognise; the dispatcher falls through to this generic warning so
the editor still sees a marker. The remediation is to upgrade the
LSP server to a build that knows about the new variant.

## Source label

Every emitted diagnostic carries `source: "aozora-lsp"` so editors
that aggregate from multiple LSPs can attribute the diagnostic to
this server.
