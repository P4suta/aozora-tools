//! `textDocument/hover` — gaiji (外字) reference resolution.
//!
//! When the cursor sits inside a `※［＃description、mencode］` (or the
//! `U+XXXX` variant) token, returns a Markdown block that shows the
//! resolved character (via `afm_encoding::gaiji::resolve`), the raw
//! description, and the mencode. Misses (cursor not in a gaiji span,
//! malformed body) return `None` and the editor falls back to no hover.
//!
//! This intentionally re-parses a small surrounding window rather than
//! asking the full afm lexer: the hover handler runs on every cursor
//! move, and the full lex + splice is ~linear in document length. The
//! small textual scan here is `O(local window)` which matters for
//! long documents. The trade-off is that malformed input outside the
//! window is ignored — we'd rather show nothing than a wrong
//! resolution.

use afm_encoding::gaiji::{self, Resolution};
use afm_syntax::Gaiji;
use tower_lsp::lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use crate::position::{byte_offset_to_position, position_to_byte_offset};

const GAIJI_OPEN: &str = "※［＃";
const GAIJI_CLOSE: &str = "］";

/// Compute a hover, if any, at `position` in `source`.
#[must_use]
pub fn hover_at(source: &str, position: Position) -> Option<Hover> {
    let byte_offset = position_to_byte_offset(source, position)?;
    let span = find_gaiji_span(source, byte_offset)?;
    let body = &source[span.start + GAIJI_OPEN.len()..span.end - GAIJI_CLOSE.len()];
    let (description, mencode) = parse_gaiji_body(body);
    let node = Gaiji {
        description: description.clone().into_boxed_str(),
        ucs: None,
        mencode: mencode.clone().map(String::into_boxed_str),
    };
    let resolution = gaiji::resolve(&node);
    let markdown = render_markdown(&description, mencode.as_deref(), &resolution);
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown,
        }),
        range: Some(Range::new(
            byte_offset_to_position(source, span.start),
            byte_offset_to_position(source, span.end),
        )),
    })
}

/// Byte-range of a `※［＃…］` span that contains `byte_offset`, or
/// `None` if no such span exists around the cursor.
///
/// Walks every `※［＃` occurrence so that a cursor sitting exactly on
/// `※` itself (byte offset == span start) still matches — the earlier
/// `rfind`-on-prefix approach missed that boundary because the prefix
/// ending at `byte_offset` did not yet contain the full trigram.
fn find_gaiji_span(source: &str, byte_offset: usize) -> Option<std::ops::Range<usize>> {
    for (start, _) in source.match_indices(GAIJI_OPEN) {
        let after_open = start + GAIJI_OPEN.len();
        let Some(end_rel) = source.get(after_open..).and_then(|s| s.find(GAIJI_CLOSE)) else {
            continue;
        };
        let end = after_open + end_rel + GAIJI_CLOSE.len();
        if (start..end).contains(&byte_offset) {
            return Some(start..end);
        }
    }
    None
}

/// Split a gaiji body (`「description」、mencode[、page-line]`) into
/// `(description, mencode?)`. The Aozora annotation manual attaches
/// optional trailing fields after the mencode (page-line references
/// for `U+XXXX` mencodes); they are informational only, so we
/// pick the first comma-separated entry after the description.
fn parse_gaiji_body(body: &str) -> (String, Option<String>) {
    let body = body.trim();
    let (description, rest) = body.find('「').map_or_else(
        || (body.to_owned(), ""),
        |open_idx| {
            let after_open = &body[open_idx + '「'.len_utf8()..];
            after_open.find('」').map_or_else(
                || (body.to_owned(), ""),
                |close_rel| {
                    let desc = after_open[..close_rel].to_owned();
                    let rest = &after_open[close_rel + '」'.len_utf8()..];
                    (desc, rest)
                },
            )
        },
    );
    let rest = rest.trim_start_matches('、').trim();
    let mencode = rest
        .split('、')
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    (description, mencode)
}

fn render_markdown(description: &str, mencode: Option<&str>, resolution: &Resolution) -> String {
    let mut md = String::from("**外字 (gaiji)**\n\n");
    if let Some(ch) = resolution.character {
        md.push_str(&format!("- 解決: `{ch}` (U+{:04X})\n", ch as u32));
    } else {
        md.push_str("- 解決: (辞書にマッチせず — 記述で代替表示)\n");
    }
    md.push_str(&format!("- 記述: `{description}`\n"));
    if let Some(m) = mencode {
        md.push_str(&format!("- mencode: `{m}`\n"));
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_on_gaiji_returns_markdown_with_resolved_char() {
        let src = "語※［＃「木＋吶のつくり」、第3水準1-85-54］で";
        // byte offset 6 は「※」の次、gaiji span 内
        let pos = byte_offset_to_position(src, 6);
        let hover = hover_at(src, pos).expect("hover should resolve");
        let md = match hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!("expected markdown hover"),
        };
        assert!(md.contains("外字"), "hover missing 外字 header: {md}");
        assert!(md.contains("木＋吶のつくり"), "hover missing description: {md}");
        // 第3水準1-85-54 → 榁 (U+6903)
        assert!(
            md.contains("榁") || md.contains("6903"),
            "hover missing resolved character U+6903 (榁): {md}",
        );
    }

    #[test]
    fn hover_on_u_plus_form_resolves_codepoint() {
        let src = "※［＃「description」、U+01F5］";
        let pos = byte_offset_to_position(src, 3);
        let hover = hover_at(src, pos).expect("hover on U+ form");
        let md = match hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!(),
        };
        assert!(md.contains("01F5") || md.contains('\u{01F5}'));
    }

    #[test]
    fn hover_outside_gaiji_returns_none() {
        let src = "ただの文です";
        let pos = Position::new(0, 2);
        assert!(hover_at(src, pos).is_none());
    }

    #[test]
    fn hover_before_gaiji_returns_none() {
        // `abc` 部分にカーソル (offset 0-2) があれば None
        let src = "abc※［＃「木＋吶のつくり」、第3水準1-85-54］で";
        let pos = byte_offset_to_position(src, 1);
        assert!(hover_at(src, pos).is_none());
    }

    #[test]
    fn hover_on_unresolved_gaiji_still_returns_markdown() {
        // 辞書未登録の mencode は character=None で返るが、hover 自体は出す
        let src = "※［＃「未知字」、第9水準9-99-99］";
        let pos = byte_offset_to_position(src, 3);
        let hover = hover_at(src, pos).expect("hover should fire even if unresolved");
        let md = match hover.contents {
            HoverContents::Markup(m) => m.value,
            _ => panic!(),
        };
        assert!(md.contains("辞書にマッチせず"));
        assert!(md.contains("未知字"));
    }
}
