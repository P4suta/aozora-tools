//! `aozora-fmt` library: the single public entry [`format_source`] runs
//! the `parse ∘ serialize` round-trip that produces an idempotent,
//! canonicalised aozora document.
//!
//! Wraps `aozora::Document::parse` + `AozoraTree::serialize` so every
//! consumer — the `aozora-fmt` binary, the `aozora-lsp` formatting
//! handler, downstream CI gates — reaches the same canonical form.
//! The round-trip is guaranteed to be a fixed point on the second
//! pass; the aozora repo's corpus sweep I3 invariant hard-gates the
//! contract across the full Aozora Bunko corpus.

#![forbid(unsafe_code)]

use aozora::Document;

/// Canonicalise an aozora source string.
///
/// Runs the aozora-lex pipeline and then the inverse serializer.
/// The returned `String` is byte-identical on the second pass.
#[must_use]
pub fn format_source(source: &str) -> String {
    Document::new(source).parse().serialize()
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
