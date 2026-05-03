# samples/

Hand-written aozora-flavored markdown documents used for manual
smoke-testing of `aozora-fmt` and `aozora-lsp`, plus the bench
fixture for `aozora-lsp/benches/burst.rs`.

| File | Exercises |
|---|---|
| `ruby.afm`                | Explicit and implicit ruby delimiters |
| `bouten.afm`              | Forward-reference bouten (`［＃「X」に傍点］`); also drives the LSP burst bench (~6 MB) |
| `gaiji.afm`               | JIS X 0213 mencode gaiji + `U+XXXX` form, smallest case |
| `gaiji-full.afm`          | Every JIS X 0213 plane × row × cell mencode the encoder resolves |
| `headings-and-breaks.afm` | Heading hints, ruby inside body text, page break |

`samples/tsumi-to-batsu-x100.afm` is a dev-only ~200 MB stress
fixture (Tsumi to Batsu × 100), gitignored and built locally from
the public Aozora Bunko text. The
[handbook's State model chapter](https://p4suta.github.io/aozora-tools/lsp/state-model.html)
describes the worst-case workloads it exists to exercise; the deeper
background on the rope-buffer decision lives in
[`docs/adr/0006-rope-buffer.md`](../docs/adr/0006-rope-buffer.md).

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
