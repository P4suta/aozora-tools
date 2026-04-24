//! `aozora-fmt` library: the single public entry [`format_source`] runs
//! the `parse ∘ serialize` round-trip that produces an idempotent,
//! canonicalised aozora document.
//!
//! This wraps `afm_parser::parse` + `afm_parser::serialize` (the
//! underlying parser crate — see ADR-0009 in the sibling `afm` repo
//! for the naming split) so every consumer — the `aozora-fmt` binary,
//! the `aozora-lsp` formatting handler, downstream CI gates — reaches
//! the same canonical form. The round-trip is guaranteed to be a
//! fixed point on the second pass; the afm repository's corpus sweep
//! I3 invariant hard-gates the contract across 17 k real Aozora works.

#![forbid(unsafe_code)]

use afm_parser::{ComrakArena, Options, parse, serialize};

/// Canonicalise an aozora source string.
///
/// Runs the full afm parser pipeline (Aozora lexer → comrak →
/// post-process splice) and then the inverse serializer. The returned
/// `String` is byte-identical on the second pass.
#[must_use]
pub fn format_source(source: &str) -> String {
    let arena = ComrakArena::new();
    let opts = Options::afm_default();
    let result = parse(&arena, source, &opts);
    serialize(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_formats_to_empty() {
        assert_eq!(format_source(""), "");
    }

    #[test]
    fn plain_text_passes_through_unchanged() {
        let input = "hello world\n";
        assert_eq!(format_source(input), input);
    }

    #[test]
    fn format_is_idempotent_on_ruby() {
        let input = "｜青梅《おうめ》へ";
        let once = format_source(input);
        let twice = format_source(&once);
        assert_eq!(once, twice, "second pass must be byte-identical");
    }

    #[test]
    fn implicit_ruby_canonicalises_to_explicit_delimiter() {
        // `日本《にほん》` (implicit) normalises to `｜日本《にほん》` (explicit).
        // The two forms parse to the same AST; we canonicalise on explicit
        // because downstream tooling (search, diff, hover) is easier when
        // the base run is always explicitly delimited.
        let implicit = "日本《にほん》";
        let formatted = format_source(implicit);
        assert!(
            formatted.starts_with('｜'),
            "expected explicit ruby delimiter, got {formatted:?}",
        );
    }

    #[test]
    fn format_is_idempotent_on_bouten() {
        let input = "彼は可哀想［＃「可哀想」に傍点］と言った";
        let once = format_source(input);
        let twice = format_source(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn format_is_idempotent_on_page_break() {
        let input = "前\n［＃改ページ］\n後\n";
        let once = format_source(input);
        let twice = format_source(&once);
        assert_eq!(once, twice);
    }
}
