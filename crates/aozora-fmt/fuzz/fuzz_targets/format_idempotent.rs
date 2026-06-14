#![no_main]
//! Coverage-guided sibling of the `format_source` proptest.
//!
//! Asserts the two properties the formatter contractually guarantees and
//! that `aozora-fmt --write` (and the LSP formatting handler) rely on:
//!
//! * **No panic** for any input — a library panic is a crash for every
//!   embedder.
//! * **Idempotence** — `format(format(x)) == format(x)`. A counter-
//!   example means the formatter would rewrite its own canonical output.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The formatter is defined over UTF-8 text. Lossy-decode so every
    // byte string is a usable input (invalid sequences become U+FFFD,
    // which the formatter must also handle gracefully).
    let text = String::from_utf8_lossy(data);
    let once = aozora_fmt::format_source(&text);
    let twice = aozora_fmt::format_source(&once);
    assert_eq!(
        once, twice,
        "format_source is not idempotent (second pass changed the output)"
    );
});
