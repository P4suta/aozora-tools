//! Per-paragraph document model.
//!
//! A document is a list of paragraphs (separated by `\n\n` runs).
//! Each paragraph owns its own [`Rope`] text + tree-sitter [`Tree`] +
//! extracted [`GaijiSpan`] table + per-paragraph
//! [`crate::line_index::LineIndex`]. Editing a single paragraph never
//! touches another paragraph's memory at all — Rope mutations stay
//! local, the tree-sitter reparse covers only that paragraph's
//! ~1-10 KB span (vs the prior whole-doc 220 ms reparse), and the
//! snapshot rebuild for unchanged paragraphs is a single `Arc` pointer
//! bump.
//!
//! See ADR-0008 for the wider rationale (paragraph-first
//! rearchitecture follow-up to ADR-0001 / ADR-0002 / ADR-0007).
//!
//! ## Structure
//!
//! - [`MutParagraph`] — writers' single-paragraph state. Lives
//!   inside `BufferState::paragraphs`. Mutable text + tree.
//! - [`ParagraphSnapshot`] — readers' single-paragraph view.
//!   Immutable, `Arc`-shareable across snapshot generations.
//!   Carries pre-computed line index + gaiji spans (with
//!   doc-absolute byte offsets).
//! - [`paragraph_byte_ranges`] — the splitter: takes a `&Rope` and
//!   returns the byte ranges of each paragraph (`\n\n` boundaries).
//! - [`build_paragraph_snapshot`] — promote a `MutParagraph` plus
//!   its byte-range-in-doc into a fully-populated immutable
//!   `ParagraphSnapshot`.
//!
//! ## Boundary policy
//!
//! Split at every `\n\n` run. The first newline goes to the LEFT
//! paragraph; the second newline starts the RIGHT paragraph. This
//! preserves byte-for-byte equality with the source when paragraphs
//! are concatenated. Empty rope yields one empty paragraph
//! `0..0`. Hard-cap at [`MAX_PARAGRAPH_BYTES`] so a never-blank-line
//! input still produces bounded segments.

use std::ops::Range;
use std::sync::Arc;

use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::gaiji_spans::{GaijiSpan, extract_gaiji_spans_from_tree};
use crate::line_index::LineIndex;

/// Hard cap on paragraph size. A never-blank-line input still
/// produces paragraphs no bigger than this; subsequent edits that
/// would grow a paragraph past the cap trigger a re-segmentation.
pub const MAX_PARAGRAPH_BYTES: usize = 64 * 1024;

/// Mutable single-paragraph state. Owned by [`crate::state::BufferState`].
#[derive(Debug)]
pub struct MutParagraph {
    pub text: Rope,
    pub tree: Option<Tree>,
}

impl MutParagraph {
    /// Build from owned text + a fresh tree (cold-start path).
    /// Caller is responsible for invoking the parser; we don't hold
    /// one because the parser lives on `BufferState` (one per doc).
    #[must_use]
    pub fn new(text: Rope) -> Self {
        Self { text, tree: None }
    }

    /// Replace the tree with a freshly-parsed one against the
    /// current text. Centralised so callers pre/post-edit go
    /// through the same chunked-input callback.
    pub fn reparse(&mut self, parser: &mut Parser) {
        self.tree = parser.parse_with_options(&mut chunk_callback(&self.text), None, None);
    }

    /// Apply an `InputEdit` against the paragraph-local tree, then
    /// reparse via chunked input. The caller has already mutated
    /// `self.text`; the `InputEdit`'s byte offsets are
    /// **paragraph-local** (NOT doc-absolute).
    pub fn apply_edit(&mut self, parser: &mut Parser, edit: InputEdit) {
        let mut prior = self.tree.take();
        if let Some(tree) = prior.as_mut() {
            tree.edit(&edit);
        }
        self.tree =
            parser.parse_with_options(&mut chunk_callback(&self.text), prior.as_ref(), None);
    }

    /// Tree id (root node id) — stable identifier for "is this the
    /// same tree as before". Used by the snapshot rebuild to skip
    /// walks for unchanged paragraphs.
    #[must_use]
    pub fn tree_id(&self) -> Option<usize> {
        self.tree.as_ref().map(|t| t.root_node().id())
    }
}

/// Immutable per-paragraph snapshot. Held inside `Arc` inside
/// [`crate::state::Snapshot::paragraphs`]. Carries the data each LSP
/// request handler needs to operate against this paragraph without
/// touching the writer side.
///
/// ## Coordinate frames
///
/// **Two coordinate frames live in this struct on purpose:**
///
/// - `byte_range`, `gaiji_spans[*].start_byte`, `gaiji_spans[*].end_byte`
///   are **document-absolute**. Handlers can hand these to
///   `LineIndex::position(text, byte)` against a doc-wide source
///   without knowing which paragraph contains them.
/// - `text`, `line_index`, `tree` reflect this paragraph's content
///   in **paragraph-local** coordinates (the tree-sitter parse was
///   fed only this paragraph's bytes; the line index counts only
///   this paragraph's `\n`s; the text starts at byte 0). Consumers
///   that mix in document-absolute positions translate via
///   `byte_range.start`.
///
/// Mixing the two is intentional — `gaiji_spans` is a pre-shifted
/// convenience for the most-common consumer (LSP `gaiji_spans` /
/// inlay), while keeping `tree` paragraph-local lets us share the
/// `Arc<Tree>` across snapshot generations untouched even when the
/// paragraph's absolute position shifted.
#[derive(Debug)]
pub struct ParagraphSnapshot {
    /// Document-absolute byte range this paragraph occupies in the
    /// containing snapshot.
    pub byte_range: Range<usize>,
    /// The paragraph's text, materialised once at snapshot build
    /// time so handlers can take `&str` slices without rope
    /// chunk-walking. **Paragraph-local** coordinates.
    pub text: Arc<str>,
    /// Per-paragraph line index. **Paragraph-local** line / column
    /// numbers. Document-level line numbers are recovered by adding
    /// the cumulative paragraph-newline counts that
    /// [`crate::semantic_tokens::semantic_tokens_full`] tracks while
    /// iterating paragraphs.
    pub line_index: Arc<LineIndex>,
    /// Gaiji spans within this paragraph, with **doc-absolute**
    /// `start_byte` / `end_byte` so handlers don't need to know
    /// which paragraph owns a span to translate it.
    pub gaiji_spans: Arc<[Arc<GaijiSpan>]>,
    /// Tree-sitter tree for this paragraph (cheap shallow `Arc`
    /// clone of the buffer-side tree). Available to wait-free
    /// readers that need structural information (semantic tokens,
    /// future linked editing). **Paragraph-local** byte coordinates;
    /// consumers shift via `byte_range.start`.
    pub tree: Option<Tree>,
    /// Snapshot of the tree's root id at build time. Re-using a
    /// `ParagraphSnapshot` across snapshot generations is keyed on
    /// `tree_id` matching the live paragraph's tree id (see
    /// `crate::state::Snapshot::rebuild_now`).
    pub tree_id: Option<usize>,
}

/// Build an immutable [`ParagraphSnapshot`] from a mutable paragraph
/// plus its document-absolute byte offset.
#[must_use]
pub fn build_paragraph_snapshot(paragraph: &MutParagraph, byte_offset: usize) -> ParagraphSnapshot {
    let text_string = paragraph.text.to_string();
    let len = text_string.len();
    let line_index = LineIndex::new(&text_string);
    let text: Arc<str> = Arc::from(text_string);
    let gaiji_spans: Arc<[Arc<GaijiSpan>]> = paragraph.tree.as_ref().map_or_else(
        || Arc::from(Vec::new()),
        |t| {
            let local = extract_gaiji_spans_from_tree(t, &text);
            shift_to_absolute(&local, byte_offset)
        },
    );
    ParagraphSnapshot {
        byte_range: byte_offset..byte_offset.saturating_add(len),
        text,
        line_index: Arc::new(line_index),
        gaiji_spans,
        tree: paragraph.tree.clone(),
        tree_id: paragraph.tree_id(),
    }
}

/// Translate every span's byte offsets from paragraph-local to
/// doc-absolute by adding `offset` to `start_byte` / `end_byte`.
/// Pointer-bumps the description / mencode `Arc<str>` fields.
fn shift_to_absolute(spans: &[Arc<GaijiSpan>], offset: usize) -> Arc<[Arc<GaijiSpan>]> {
    let off = u32::try_from(offset).unwrap_or(u32::MAX);
    let out: Vec<Arc<GaijiSpan>> = spans
        .iter()
        .map(|s| {
            Arc::new(GaijiSpan {
                start_byte: s.start_byte.saturating_add(off),
                end_byte: s.end_byte.saturating_add(off),
                description: Arc::clone(&s.description),
                mencode: s.mencode.clone(),
            })
        })
        .collect();
    out.into()
}

impl ParagraphSnapshot {
    /// Produce an `Arc<ParagraphSnapshot>` placed at `new_start`.
    ///
    /// Operates on `&Arc<Self>` (not `&Self`) so the no-shift path
    /// can return `Arc::clone(prior)` — a single atomic increment
    /// with zero allocations. The shift path allocates a new
    /// `Arc<Self>` whose `text` / `line_index` / `tree` fields are
    /// `Arc::clone`d from the prior (sharing memory across snapshot
    /// generations) and whose gaiji-span list is rebuilt with
    /// shifted offsets.
    ///
    /// Snapshot rebuilds use this for paragraphs whose tree didn't
    /// change but whose absolute position shifted because a
    /// preceding paragraph grew or shrank.
    #[must_use]
    pub fn shifted_to(prior: &Arc<Self>, new_start: usize) -> Arc<Self> {
        if prior.byte_range.start == new_start {
            // No shift — pure pointer bump, no work at all.
            return Arc::clone(prior);
        }
        let len = prior.byte_range.len();
        let prior_start = prior.byte_range.start;
        let new_spans = shift_existing_spans(&prior.gaiji_spans, prior_start, new_start);
        Arc::new(Self {
            byte_range: new_start..new_start.saturating_add(len),
            text: Arc::clone(&prior.text),
            line_index: Arc::clone(&prior.line_index),
            gaiji_spans: new_spans,
            tree: prior.tree.clone(),
            tree_id: prior.tree_id,
        })
    }
}

fn shift_existing_spans(
    spans: &[Arc<GaijiSpan>],
    prior_start: usize,
    new_start: usize,
) -> Arc<[Arc<GaijiSpan>]> {
    if prior_start == new_start {
        // Same position — every span's bytes stay; just clone the
        // outer Arc<[...]> fat pointer and let the inner Arc<GaijiSpan>
        // entries pointer-bump via `Vec::from(slice).into()`.
        return spans.to_vec().into();
    }
    let new_signed = i64::try_from(new_start).unwrap_or(i64::MAX);
    let prior_signed = i64::try_from(prior_start).unwrap_or(i64::MAX);
    let delta_signed = new_signed - prior_signed;
    let out: Vec<Arc<GaijiSpan>> = spans
        .iter()
        .map(|s| {
            let new_start_byte = i64::from(s.start_byte) + delta_signed;
            let new_end_byte = i64::from(s.end_byte) + delta_signed;
            Arc::new(GaijiSpan {
                start_byte: u32::try_from(new_start_byte).unwrap_or(u32::MAX),
                end_byte: u32::try_from(new_end_byte).unwrap_or(u32::MAX),
                description: Arc::clone(&s.description),
                mencode: s.mencode.clone(),
            })
        })
        .collect();
    out.into()
}

/// Compute paragraph byte ranges over `rope` per the
/// "split at `\n\n` runs" policy. Empty rope yields `[0..0]`.
/// Worst case (no blank lines for a long stretch) hard-splits at
/// the [`MAX_PARAGRAPH_BYTES`] cap.
#[must_use]
pub fn paragraph_byte_ranges(rope: &Rope) -> Vec<Range<usize>> {
    let total = rope.len_bytes();
    let mut out: Vec<Range<usize>> = Vec::new();
    if total == 0 {
        out.push(0..0);
        return out;
    }
    let bytes: Vec<u8> = rope.bytes().collect();
    let mut start = 0usize;
    while start < total {
        let mut end = start;
        let mut soft_end: Option<usize> = None;
        while end < total {
            // Cap fallback.
            if end - start >= MAX_PARAGRAPH_BYTES {
                end = soft_end.unwrap_or(end).max(start + 1);
                break;
            }
            // \n\n boundary: include the first \n in this segment;
            // the next iteration starts at the second \n.
            if bytes[end] == b'\n' && end + 1 < total && bytes[end + 1] == b'\n' {
                end += 1;
                break;
            }
            // Single \n is a soft boundary candidate for the cap path.
            if bytes[end] == b'\n' {
                soft_end = Some(end + 1);
            }
            end += 1;
        }
        out.push(start..end.min(total));
        start = end.min(total);
    }
    if out.is_empty() {
        out.push(0..total);
    }
    out
}

/// Tree-sitter chunked-input callback over a paragraph's local
/// `Rope`. Used by both the cold-start and incremental parse
/// paths.
pub fn chunk_callback<'r>(rope: &'r Rope) -> impl FnMut(usize, Point) -> &'r [u8] {
    let len = rope.len_bytes();
    move |byte_idx, _pos| -> &'r [u8] {
        if byte_idx >= len {
            return &[];
        }
        let (chunk, chunk_byte_idx, _, _) = rope.chunk_at_byte(byte_idx);
        let local = byte_idx - chunk_byte_idx;
        &chunk.as_bytes()[local..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rope(s: &str) -> Rope {
        Rope::from(s)
    }

    #[test]
    fn paragraph_byte_ranges_empty_yields_one_empty_range() {
        let r = rope("");
        let ranges = paragraph_byte_ranges(&r);
        assert_eq!(ranges, vec![0..0]);
    }

    #[test]
    fn paragraph_byte_ranges_single_paragraph_no_split() {
        let r = rope("ただの一段落だけ");
        let ranges = paragraph_byte_ranges(&r);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..r.len_bytes());
    }

    #[test]
    fn paragraph_byte_ranges_splits_on_blank_line() {
        let s = "段落1\n\n段落2";
        let r = rope(s);
        let ranges = paragraph_byte_ranges(&r);
        assert_eq!(ranges.len(), 2);
        let blank_at = s.find("\n\n").unwrap();
        assert_eq!(ranges[0].end, blank_at + 1);
        assert_eq!(ranges[1].start, blank_at + 1);
        assert_eq!(ranges[1].end, s.len());
    }

    #[test]
    fn paragraph_byte_ranges_cover_full_rope_with_no_gaps() {
        let s = "一\n\n二\n\n三";
        let r = rope(s);
        let ranges = paragraph_byte_ranges(&r);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, s.len());
        for w in ranges.windows(2) {
            assert_eq!(w[0].end, w[1].start, "no gap or overlap");
        }
    }

    #[test]
    fn build_snapshot_extracts_local_gaiji_at_doc_absolute_offset() {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .unwrap();
        let mut p = MutParagraph::new(rope("※［＃「a」、X］"));
        p.reparse(&mut parser);
        let snap = build_paragraph_snapshot(&p, 1000);
        assert_eq!(snap.byte_range.start, 1000);
        assert_eq!(snap.gaiji_spans.len(), 1);
        let span = &snap.gaiji_spans[0];
        assert_eq!(span.start_byte, 1000); // local 0 + 1000 offset
        assert_eq!(&*span.description, "a");
    }
}
