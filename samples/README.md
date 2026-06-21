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
| `diagnostics.afm`         | A stray `］` that `aozora lint`/`explain` flag (`aozora::unmatched-close`) — canonical on disk, so `fmt --check` still passes |

`samples/tsumi-to-batsu-x100.afm` is a dev-only ~200 MB stress
fixture (Tsumi to Batsu × 100), gitignored and built locally from
the public Aozora Bunko text. The
[handbook's State model chapter](https://p4suta.github.io/aozora-tools/lsp/state-model.html)
describes the worst-case workloads it exists to exercise.

## Try

```bash
# Canonicalised form to stdout
cargo run --bin aozora -- fmt samples/ruby.afm

# Diff-style check against the on-disk form
cargo run --bin aozora -- fmt --check samples/ruby.afm

# Terminal diagnostics (rustc-style), then explain a code
cargo run --bin aozora -- lint samples/diagnostics.afm
cargo run --bin aozora -- explain aozora::unmatched-close

# Render a document to HTML
cargo run --bin aozora -- render samples/ruby.afm
```

The files are kept canonical on disk — `aozora fmt --check` should
exit 0 for every one of them (including `diagnostics.afm`, whose lone
defect is a stray delimiter the formatter leaves byte-for-byte intact).
If that stops being true after a parser change, commit the regeneration
so diffs stay reviewable.
