//! `textDocument/inlayHint` handler — cache-driven, lock-free reads.
//!
//! Walks the pre-extracted [`crate::gaiji_spans::GaijiSpan`] list
//! kept up-to-date by `DocState` (refreshed under the same write
//! lock as the text edit) and emits a small "→ glyph" inlay just
//! *after* every `※［＃…］` gaiji span whose description+mencode
//! resolve through [`aozora_encoding::gaiji::lookup`].
//!
//! ## Why cache-driven
//!
//! Earlier passes walked the tree-sitter tree per request, holding
//! the parser `Mutex` for the duration. Concurrent inlay calls
//! (VS Code fires several per cursor move / viewport change)
//! serialised on that lock, costing tens-to-hundreds of milliseconds
//! per burst. The cached span list lives behind an
//! `Arc<[GaijiSpan]>` that the handler clones — concurrent calls
//! work against independent immutable snapshots.
//!
//! Combined with [`crate::line_index::LineIndex`], the per-hint cost
//! drops from `O(doc_size)` (full byte walk per position conversion)
//! to `O(log lines)`. The whole handler is now
//! `O(log lines + visible spans)` — independent of document size.

use aozora_encoding::gaiji;
use tower_lsp::lsp_types::{
    InlayHint, InlayHintKind, InlayHintLabel, InlayHintLabelPart, InlayHintLabelPartTooltip,
    MarkupContent, MarkupKind, Range,
};

use crate::gaiji_spans::{GaijiSpan, spans_in_byte_range};
use crate::line_index::LineIndex;

/// Compute every inlay hint inside `range` (in LSP coordinates).
/// Spans are filtered to the requested viewport via binary search;
/// each surviving span is resolved through `gaiji::lookup` and
/// formatted into an `InlayHint`.
#[must_use]
pub fn inlay_hints(
    source: &str,
    spans: &[GaijiSpan],
    line_index: &LineIndex,
    range: Range,
) -> Vec<InlayHint> {
    let Some(start_byte) = line_index.byte_offset(source, range.start) else {
        return Vec::new();
    };
    let Some(end_byte) = line_index.byte_offset(source, range.end) else {
        return Vec::new();
    };
    let visible = spans_in_byte_range(spans, start_byte, end_byte);
    visible
        .iter()
        .filter_map(|span| build_hint(span, source, line_index))
        .collect()
}

fn build_hint(span: &GaijiSpan, source: &str, line_index: &LineIndex) -> Option<InlayHint> {
    let resolved = gaiji::lookup(None, span.mencode.as_deref(), &span.description)?;

    let mut display = String::new();
    let _ = resolved.write_to(&mut display);
    let codepoints: String = display
        .chars()
        .map(|c| format!("U+{:04X}", c as u32))
        .collect::<Vec<_>>()
        .join(" + ");
    let pos = line_index.position(source, span.end_byte as usize);
    Some(InlayHint {
        position: pos,
        label: InlayHintLabel::LabelParts(vec![InlayHintLabelPart {
            value: format!(" → {display}"),
            tooltip: Some(InlayHintLabelPartTooltip::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!(
                    "**外字**: `{}`\n\n- 解決: `{display}` ({codepoints})",
                    span.description,
                ),
            })),
            location: None,
            command: None,
        }]),
        kind: Some(InlayHintKind::TYPE),
        text_edits: None,
        tooltip: None,
        padding_left: Some(true),
        padding_right: Some(false),
        data: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gaiji_spans::extract_gaiji_spans;
    use tower_lsp::lsp_types::Position;
    use tree_sitter::Parser;

    fn parse_spans(src: &str) -> std::sync::Arc<[GaijiSpan]> {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(src, None).unwrap();
        extract_gaiji_spans(&tree, src)
    }

    fn full_range(src: &str, idx: &LineIndex) -> Range {
        Range::new(Position::new(0, 0), idx.position(src, src.len()))
    }

    #[test]
    fn emit_one_hint_per_resolved_gaiji() {
        let src = "前※［＃「木＋吶のつくり」、第3水準1-85-54］後";
        let spans = parse_spans(src);
        let idx = LineIndex::new(src);
        let hints = inlay_hints(src, &spans, &idx, full_range(src, &idx));
        assert_eq!(hints.len(), 1, "{hints:?}");
        let label = format!("{:?}", hints[0].label);
        assert!(label.contains("6798"), "{label}");
    }

    #[test]
    fn skip_unresolved_gaiji() {
        let src = "※［＃「unknown」、第9水準9-99-99］";
        let spans = parse_spans(src);
        let idx = LineIndex::new(src);
        let hints = inlay_hints(src, &spans, &idx, full_range(src, &idx));
        assert!(hints.is_empty(), "{hints:?}");
    }

    #[test]
    fn out_of_range_hints_are_filtered() {
        let src = "※［＃「A」、第3水準1-85-54］\n※［＃「B」、第3水準1-85-54］";
        let spans = parse_spans(src);
        let idx = LineIndex::new(src);
        let line2_only = Range::new(Position::new(1, 0), Position::new(1, 100));
        let hints = inlay_hints(src, &spans, &idx, line2_only);
        assert_eq!(hints.len(), 1, "{hints:?}");
    }

    #[test]
    fn plain_text_emits_no_hints() {
        let src = "ただの文章";
        let spans = parse_spans(src);
        let idx = LineIndex::new(src);
        assert!(inlay_hints(src, &spans, &idx, full_range(src, &idx)).is_empty());
    }
}
