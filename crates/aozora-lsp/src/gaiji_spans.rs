//! Pre-extracted gaiji span list.
//!
//! ## Why this exists
//!
//! `inlay_hints` (and a future "rename gaiji description" code action,
//! and a "next/previous gaiji" navigation command) all want the same
//! thing: every `※［＃...］` span in the document, plus its
//! description and mencode bytes. Repeatedly walking the tree-sitter
//! tree per request:
//!
//! - holds the parser `Mutex` for the duration of the walk —
//!   serialising concurrent inlay requests, even though the read is
//!   conceptually pure;
//! - allocates fresh `String`s for description / mencode every time;
//! - costs the same byte-walk on every range query, even though the
//!   underlying source hasn't changed.
//!
//! [`GaijiSpan`] caches the per-span data once per text version,
//! built immediately after the tree-sitter incremental update lands.
//! Reads are lock-free `Arc<[GaijiSpan]>` clones, and the inlay
//! handler does a binary-search range filter on `start_byte` instead
//! of walking the whole tree.
//!
//! The cache is invalidated on every `did_change` and rebuilt under
//! the same `DashMap` write lock that already guards the text edit;
//! refresh cost is `O(gaiji count)` and so is bounded by document
//! size, not request rate.

use std::sync::Arc;

use tree_sitter::{Node, Tree};
use tree_sitter_aozora::kind;

/// One `※［＃description、mencode］` occurrence.
///
/// Owns its `description` / `mencode` strings (small per-span
/// allocations, ~30 bytes each) so the cache survives source
/// edits without dangling lifetimes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaijiSpan {
    /// Byte offset of the leading `※` (or `［＃` for the bare-slug
    /// flavour a future feature might wire in here).
    pub start_byte: u32,
    /// Byte offset just past the closing `］`.
    pub end_byte: u32,
    /// Description text — the bit between `「` and `」`, brackets
    /// stripped. Used as the resolution key for the gaiji-chuki
    /// dictionary fallback.
    pub description: String,
    /// `第3水準1-85-54` / `U+1234` / similar — `None` if the source
    /// omitted the mencode tail.
    pub mencode: Option<String>,
}

/// Walk `tree` once and extract every gaiji span. Output is sorted
/// by `start_byte` (the tree walk is in source order).
#[must_use]
pub fn extract_gaiji_spans(tree: &Tree, source: &str) -> Arc<[GaijiSpan]> {
    let mut spans = Vec::new();
    walk(tree.root_node(), source, &mut spans);
    spans.into()
}

fn walk(node: Node<'_>, source: &str, out: &mut Vec<GaijiSpan>) {
    if node.kind() == kind::GAIJI {
        if let Some(span) = build_span(node, source) {
            out.push(span);
        }
        // Don't descend — gaiji nodes are leaves for this walk.
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, source, out);
    }
}

fn build_span(gaiji: Node<'_>, source: &str) -> Option<GaijiSpan> {
    // gaiji wraps a slug whose `body` field carries
    // `description、mencode`.
    let mut cursor = gaiji.walk();
    let slug = gaiji
        .named_children(&mut cursor)
        .find(|c| c.kind() == kind::SLUG)?;
    let body_node = slug.child_by_field_name("body")?;
    let body_text = body_node.utf8_text(source.as_bytes()).ok()?;
    let (description, mencode) = parse_body(body_text);
    Some(GaijiSpan {
        start_byte: u32::try_from(gaiji.start_byte()).unwrap_or(u32::MAX),
        end_byte: u32::try_from(gaiji.end_byte()).unwrap_or(u32::MAX),
        description,
        mencode,
    })
}

/// Split `description, mencode` from a slug body. The first `、`
/// delimits description from mencode; if absent, the whole body is
/// the description and mencode is `None`. Strips the surrounding
/// `「…」` from the description if present.
fn parse_body(body: &str) -> (String, Option<String>) {
    let (description, rest) = body.find('、').map_or((body, None), |i| {
        let head = body[..i].trim();
        let tail = body[i + '、'.len_utf8()..].trim();
        (head, Some(tail.to_owned()))
    });
    let description = description.trim().trim_matches(|c| c == '「' || c == '」');
    (description.to_owned(), rest.filter(|s| !s.is_empty()))
}

/// Filter `spans` to those whose `start_byte` lies in
/// `[start_byte, end_byte)`. Uses binary search on the sorted
/// `start_byte` field so the per-request cost stays
/// `O(log spans + matches)`.
#[must_use]
pub fn spans_in_byte_range(
    spans: &[GaijiSpan],
    start_byte: usize,
    end_byte: usize,
) -> &[GaijiSpan] {
    let start = u32::try_from(start_byte).unwrap_or(u32::MAX);
    let end = u32::try_from(end_byte).unwrap_or(u32::MAX);
    // First span whose end exceeds the start of the requested range.
    let lo = spans.partition_point(|s| s.end_byte <= start);
    // First span whose start is past the end of the requested range.
    let hi = spans.partition_point(|s| s.start_byte < end);
    &spans[lo..hi.max(lo)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(src: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .unwrap();
        parser.parse(src, None).unwrap()
    }

    #[test]
    fn empty_source_yields_no_spans() {
        let tree = parse("");
        assert!(extract_gaiji_spans(&tree, "").is_empty());
    }

    #[test]
    fn extracts_one_gaiji_span() {
        let src = "前※［＃「desc」、第3水準1-85-54］後";
        let tree = parse(src);
        let spans = extract_gaiji_spans(&tree, src);
        assert_eq!(spans.len(), 1);
        let span = &spans[0];
        assert_eq!(span.description, "desc");
        assert_eq!(span.mencode.as_deref(), Some("第3水準1-85-54"));
        assert_eq!(span.start_byte as usize, src.find('※').unwrap());
    }

    #[test]
    fn extracts_multiple_spans_in_source_order() {
        let src = "※［＃「a」、第3水準1-85-54］\n※［＃「b」、第3水準1-85-9］";
        let tree = parse(src);
        let spans = extract_gaiji_spans(&tree, src);
        assert_eq!(spans.len(), 2);
        assert!(spans[0].start_byte < spans[1].start_byte);
        assert_eq!(spans[0].description, "a");
        assert_eq!(spans[1].description, "b");
    }

    #[test]
    fn description_only_form_yields_none_mencode() {
        let src = "※［＃「desc-only」］";
        let tree = parse(src);
        let spans = extract_gaiji_spans(&tree, src);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].description, "desc-only");
        assert!(spans[0].mencode.is_none());
    }

    #[test]
    fn binary_search_filters_out_of_range_spans() {
        let src = "※［＃「a」、X］\n※［＃「b」、X］\n※［＃「c」、X］";
        let tree = parse(src);
        let spans = extract_gaiji_spans(&tree, src);
        // Restrict to second span's byte range.
        let b_start = src.find("※［＃「b").unwrap();
        let b_end = src
            .match_indices('］')
            .nth(1)
            .map(|(i, _)| i + '］'.len_utf8())
            .unwrap();
        let inside = spans_in_byte_range(&spans, b_start, b_end);
        assert_eq!(inside.len(), 1);
        assert_eq!(inside[0].description, "b");
    }
}
