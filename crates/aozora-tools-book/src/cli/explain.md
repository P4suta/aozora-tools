# Explaining diagnostics

Every diagnostic carries a stable code (e.g. `aozora::unmatched-close`) and a
`= help` line pointing at `aozora explain`. That command prints the long-form
explanation: what the diagnostic means, why it matters, and a before/after
example.

```console
$ aozora explain unmatched-close
aozora::unmatched-close

対応する開き括弧のない閉じ括弧

閉じ括弧 (`］` `》` `」` など) に対応する開き括弧が見つかりません。
…

  問題のある例:
    本文 ］

  直した例:
    本文
```

## Resolving the code

- The `aozora::` namespace is optional: `unmatched-close` and
  `aozora::unmatched-close` both resolve.
- A **unique prefix** is accepted (`unclosed` → `aozora::unclosed-bracket`).
- On a **typo**, the nearest code is suggested (exit `2`):

  ```console
  $ aozora explain unclosed-bracketx
  aozora explain: unknown diagnostic code `unclosed-bracketx`; did you mean `aozora::unclosed-bracket`?
  ```

- With **no argument**, every code is listed:

  ```console
  $ aozora explain
  診断コード一覧 (詳細は `aozora explain <code>`):

    aozora::source-contains-pua            私用領域文字がソースに紛れ込んでいる
    aozora::unclosed-bracket               閉じられていない開き括弧
    …
  ```

The explanations are the single source of truth in the
[`aozora-diagnostics`](https://github.com/P4suta/aozora-tools/tree/main/crates/aozora-diagnostics)
crate, shared with the LSP's [diagnostics catalogue](../lsp/diagnostics.md).
