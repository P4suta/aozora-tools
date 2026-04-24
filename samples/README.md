# samples/

Hand-written afm documents used for manual smoke-testing of
`aozora-fmt` and `aozora-lsp`.

| File | Exercises |
|---|---|
| `ruby.afm`               | Explicit and implicit ruby delimiters |
| `bouten.afm`             | Forward-reference bouten (`［＃「X」に傍点］`) |
| `gaiji.afm`              | JIS X 0213 mencode gaiji + `U+XXXX` form |
| `headings-and-breaks.afm`| Heading hints, ruby inside body text, page break |

## Try

```bash
# Canonicalised form to stdout
cargo run --bin aozora-fmt -- samples/ruby.afm

# Diff-style check against the on-disk form
cargo run --bin aozora-fmt -- --check samples/ruby.afm

# In-place rewrite (no-op on already canonical input)
cargo run --bin aozora-fmt -- --write samples/ruby.afm
```

The files are kept canonical on disk — `aozora-fmt --check` should
exit 0 for every one of them. If that stops being true after a
parser change, commit the regeneration so diffs stay reviewable.
