//! Per-paragraph segmented tree-sitter state.
//!
//! ## Why
//!
//! Tree-sitter's incremental reparse, even given a near-perfect
//! `Tree::edit` + `parser.parse(text, Some(&edited))` pipeline, is
//! `O(doc-size)` on the aozora grammar — bench data
//! (`subcomponents/ts_apply_edit_*` and `ts_parse_full_*`) shows
//! ~33 ns/byte irrespective of edit position. On a 6 MB document
//! that's ~220 ms per keystroke; on a typical aozora paragraph
//! (1-10 KB) it would be ~30 μs - 330 μs. **The practical fix is to
//! cut the document into paragraph-sized segments and re-parse only
//! the segment that contains the edit.**
//!
//! Boundaries are `\n\n` runs — the canonical aozora paragraph
//! separator. This is a structural property of the input format, not
//! a tree-sitter concern, so segment edges align cleanly with the
//! grammar's `_element` repeats and don't fight the parser.
//!
//! ## State shape
//!
//! ```text
//! SegmentedDoc
//! └── segments: Vec<Segment>           // ordered by byte_range.start
//!     └── Segment
//!         ├── byte_range: Range<usize>  // absolute, in current Rope coords
//!         └── tree: Option<Tree>        // tree-sitter tree of segment text
//! ```
//!
//! Invariants:
//!
//! - `segments` is non-empty after the first `parse_full_rope`.
//! - Adjacent segments have `prev.end == next.start` (no gaps, no
//!   overlap). The `\n\n` boundary belongs to the LEFT segment
//!   (specifically, the first `\n` is included; the second starts
//!   the next segment). This keeps the union of segments equal to
//!   the source bytes byte-for-byte.
//! - Total segment span equals `rope.len_bytes()`.
//!
//! ## Edit handling
//!
//! `apply_edit_rope(rope, edit)`:
//!
//! 1. Locate the segment containing `edit.start_byte` (binary
//!    search by `start..end`). Inserts at exactly `seg.end` count
//!    as the LEFT segment, matching how the boundary belongs.
//! 2. Compute the post-edit segment span by widening the original
//!    segment's range by the edit delta (`new_end_byte -
//!    old_end_byte`). All subsequent segments shift by the same
//!    delta.
//! 3. Re-parse JUST the affected segment from
//!    `rope.slice(seg_byte_range)` via the chunked-input callback.
//!    No global reparse.
//!
//! For edits that span multiple segments (rare in keystroke-rate
//! editing), we currently fall back to a full re-segmentation:
//! take the new rope and rebuild every segment. Coarse but correct.
//!
//! ## Why not `Parser::set_included_ranges`
//!
//! Tree-sitter's `set_included_ranges` redefines what the parser
//! considers "the source" — nodes outside the included range are
//! dropped from the resulting tree. That's the wrong primitive:
//! we want N independent trees that each cover their own range,
//! not a single tree that ignores the rest of the document. Using
//! `included_ranges` would force a tree-rebuild on every shift,
//! which is what we're trying to avoid.

use std::ops::Range;
use std::sync::Mutex;

use ropey::{Rope, RopeSlice};
use tree_sitter::{InputEdit, Parser, Point, Tree};

/// Maximum segment size (bytes). Edits that grow a segment past
/// this trigger a re-segmentation — keeps the worst-case per-edit
/// reparse cost bounded.
const MAX_SEGMENT_BYTES: usize = 64 * 1024;

/// Segmented per-document tree-sitter state. See module docs.
pub struct SegmentedDoc {
    inner: Mutex<Inner>,
}

struct Inner {
    parser: Parser,
    segments: Vec<Segment>,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub byte_range: Range<usize>,
    pub tree: Option<Tree>,
}

impl std::fmt::Debug for SegmentedDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SegmentedDoc").finish_non_exhaustive()
    }
}

impl SegmentedDoc {
    /// Build a fresh `SegmentedDoc`. The parser is initialised with
    /// the bundled `tree-sitter-aozora` language.
    ///
    /// # Panics
    /// If the bundled grammar's ABI is incompatible with the linked
    /// tree-sitter runtime (only fires on build-time grammar
    /// regenerations against a different runtime than Cargo
    /// resolved).
    #[must_use]
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .expect("tree-sitter-aozora language is compiled in");
        Self {
            inner: Mutex::new(Inner {
                parser,
                segments: Vec::new(),
            }),
        }
    }

    /// Replace the segment list with a fresh segmentation of `rope`
    /// and parse every segment. Used on `didOpen` and on full
    /// document replacement.
    ///
    /// # Panics
    /// On a poisoned inner mutex.
    pub fn parse_full_rope(&self, rope: &Rope) {
        let mut inner = self.inner.lock().expect("parser mutex");
        let inner = &mut *inner;
        let ranges = segment_ranges(rope);
        inner.segments = ranges
            .into_iter()
            .map(|byte_range| Segment {
                tree: parse_segment(&mut inner.parser, rope, byte_range.clone()),
                byte_range,
            })
            .collect();
    }

    /// Apply an incremental edit. Locates the affected segment,
    /// shifts subsequent segments by the edit delta, and re-parses
    /// just the touched segment. Falls back to full re-segmentation
    /// when the edit crosses segment boundaries OR when the post-
    /// edit segment would exceed `MAX_SEGMENT_BYTES`.
    ///
    /// # Panics
    /// On a poisoned inner mutex.
    pub fn apply_edit_rope(&self, rope: &Rope, edit: InputEdit) {
        let mut inner = self.inner.lock().expect("parser mutex");
        let inner = &mut *inner;

        // Empty doc — fall back to full parse.
        if inner.segments.is_empty() {
            let ranges = segment_ranges(rope);
            inner.segments = ranges
                .into_iter()
                .map(|byte_range| Segment {
                    tree: parse_segment(&mut inner.parser, rope, byte_range.clone()),
                    byte_range,
                })
                .collect();
            return;
        }

        let delta = ByteDelta::from_edit(&edit);
        let touched = locate_segment(&inner.segments, edit.start_byte, edit.old_end_byte);

        match touched {
            TouchedSegments::Single(idx) => {
                // Widen the affected segment by the delta and shift
                // every following segment.
                let prev_range = inner.segments[idx].byte_range.clone();
                let new_end = delta.apply_clamped(prev_range.end, prev_range.start);
                let new_range = prev_range.start..new_end;

                if new_range.len() > MAX_SEGMENT_BYTES {
                    // Segment grew past the cap — re-segment.
                    full_resegment(inner, rope);
                    return;
                }

                inner.segments[idx].byte_range = new_range.clone();
                inner.segments[idx].tree = parse_segment(&mut inner.parser, rope, new_range);

                for seg in &mut inner.segments[(idx + 1)..] {
                    let shifted_start = delta.apply(seg.byte_range.start);
                    let shifted_end = delta.apply(seg.byte_range.end);
                    seg.byte_range = shifted_start..shifted_end;
                }
            }
            TouchedSegments::Multiple => {
                // Cross-segment edits force a full re-segment. Coarse
                // but correct; in keystroke-rate editing this is rare
                // (single-character inserts stay inside a paragraph).
                full_resegment(inner, rope);
            }
        }
    }

    /// Walk every segment's tree under `f`. Returns an iterator over
    /// `(segment_byte_range_start, &Tree)` pairs. Callers that
    /// emit document-level positions add `segment_byte_range_start`
    /// to each tree-internal byte offset.
    ///
    /// # Panics
    /// On a poisoned inner mutex.
    pub fn with_segments<R>(&self, f: impl FnOnce(&[Segment]) -> R) -> R {
        let inner = self.inner.lock().expect("parser mutex");
        f(&inner.segments)
    }

    /// Number of segments — diagnostic, used by tests.
    ///
    /// # Panics
    /// On a poisoned inner mutex.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.inner.lock().expect("parser mutex").segments.len()
    }
}

impl Default for SegmentedDoc {
    fn default() -> Self {
        Self::new()
    }
}

enum TouchedSegments {
    Single(usize),
    Multiple,
}

/// Signed byte delta from an `InputEdit`, with safe-cast helpers so
/// the segment-shift code doesn't need raw `as` casts that trip the
/// pedantic clippy lints.
#[derive(Clone, Copy)]
struct ByteDelta(i64);

impl ByteDelta {
    fn from_edit(edit: &InputEdit) -> Self {
        let new = i64::try_from(edit.new_end_byte).unwrap_or(i64::MAX);
        let old = i64::try_from(edit.old_end_byte).unwrap_or(i64::MAX);
        Self(new - old)
    }

    /// Shift `byte_offset` by `self`, saturating at zero on
    /// underflow. Used for normal "this offset is past the edit, just
    /// add the delta" shifts.
    fn apply(self, byte_offset: usize) -> usize {
        let signed = i64::try_from(byte_offset).unwrap_or(i64::MAX) + self.0;
        usize::try_from(signed.max(0)).unwrap_or(0)
    }

    /// Shift `byte_offset` like [`Self::apply`] but clamp the result
    /// from below to `floor`. Used when widening a segment range so
    /// `end < start` can't slip through.
    fn apply_clamped(self, byte_offset: usize, floor: usize) -> usize {
        self.apply(byte_offset).max(floor)
    }
}

/// Find which segment(s) the edit's byte range `[start, old_end)`
/// falls into. The boundary at `seg.end` belongs to the LEFT
/// segment (this matches `parse_full_rope`'s split policy).
fn locate_segment(segments: &[Segment], start: usize, old_end: usize) -> TouchedSegments {
    // Binary search for the segment containing `start`.
    let idx = segments
        .partition_point(|seg| seg.byte_range.end <= start)
        .min(segments.len().saturating_sub(1));
    let seg = &segments[idx];
    if old_end <= seg.byte_range.end {
        TouchedSegments::Single(idx)
    } else {
        TouchedSegments::Multiple
    }
}

fn full_resegment(inner: &mut Inner, rope: &Rope) {
    let ranges = segment_ranges(rope);
    inner.segments = ranges
        .into_iter()
        .map(|byte_range| Segment {
            tree: parse_segment(&mut inner.parser, rope, byte_range.clone()),
            byte_range,
        })
        .collect();
}

/// Compute the segmentation of `rope` into paragraph-sized chunks.
///
/// Boundary policy:
/// 1. Split at every `\n\n` run — the canonical aozora paragraph
///    separator. The first newline goes to the LEFT segment; the
///    second newline starts the RIGHT segment.
/// 2. If a single paragraph exceeds `MAX_SEGMENT_BYTES`, hard-split
///    it at the next `\n` boundary that fits. Worst case (no
///    newlines for a long stretch) we hard-split at the byte cap
///    even mid-line, which is rare for aozora.
/// 3. Empty rope → one empty segment `0..0`.
fn segment_ranges(rope: &Rope) -> Vec<Range<usize>> {
    let total = rope.len_bytes();
    let mut out: Vec<Range<usize>> = Vec::new();
    if total == 0 {
        out.push(0..0);
        return out;
    }
    let mut start = 0usize;
    let bytes = rope.bytes().collect::<Vec<u8>>();
    while start < total {
        let mut end = start;
        let mut soft_end: Option<usize> = None;
        while end < total {
            // Hard cap: fall back at the cap.
            if end - start >= MAX_SEGMENT_BYTES {
                let candidate = soft_end.unwrap_or(end);
                end = candidate.max(start + 1);
                break;
            }
            // Boundary: \n\n run. The first \n stays with this segment;
            // the next iteration starts at the second \n.
            if bytes[end] == b'\n' && end + 1 < total && bytes[end + 1] == b'\n' {
                end += 1; // include first \n
                break;
            }
            // Single \n is a soft boundary candidate for the cap-fallback path.
            if bytes[end] == b'\n' {
                soft_end = Some(end + 1);
            }
            end += 1;
        }
        let _ = soft_end; // consumed only via the cap-fallback path above
        out.push(start..end.min(total));
        start = end.min(total);
    }
    if out.is_empty() {
        out.push(0..total);
    }
    out
}

/// Parse a single segment by feeding the parser only the bytes
/// within `byte_range`. Uses the chunked-input callback so the
/// parser doesn't need a contiguous String materialisation.
fn parse_segment(parser: &mut Parser, rope: &Rope, byte_range: Range<usize>) -> Option<Tree> {
    if byte_range.is_empty() {
        return None;
    }
    let slice = rope.byte_slice(byte_range.clone());
    let len = byte_range.len();
    let start = byte_range.start;
    let mut callback = move |byte_idx_in_doc: usize, _pos: Point| -> &[u8] {
        // The parser's offsets are document-relative because we
        // feed it the slice via chunk_at_byte against the FULL rope
        // — but here we want segment-local. Translate.
        let local = byte_idx_in_doc.saturating_sub(start);
        if local >= len {
            return &[];
        }
        let chunk = chunk_at_in_slice(slice, local);
        // Defensive: the chunk extends past `len - local` for the
        // last chunk in the slice when chunks naturally end past
        // the slice boundary. Trim.
        let max = len - local;
        if chunk.len() > max {
            &chunk[..max]
        } else {
            chunk
        }
    };
    parser.parse_with_options(&mut callback, None, None)
}

fn chunk_at_in_slice(slice: RopeSlice<'_>, local_byte: usize) -> &[u8] {
    let (chunk, chunk_byte_idx, _, _) = slice.chunk_at_byte(local_byte);
    let local = local_byte - chunk_byte_idx;
    &chunk.as_bytes()[local..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::input_edit;

    fn rope(s: &str) -> Rope {
        Rope::from(s)
    }

    #[test]
    fn segment_ranges_empty_rope_yields_one_empty_range() {
        let r = rope("");
        let ranges = segment_ranges(&r);
        assert_eq!(ranges, vec![0..0]);
    }

    #[test]
    fn segment_ranges_single_paragraph_no_split() {
        let r = rope("ただの一段落だけ");
        let ranges = segment_ranges(&r);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], 0..r.len_bytes());
    }

    #[test]
    fn segment_ranges_splits_on_blank_line() {
        let s = "段落1\n\n段落2";
        let r = rope(s);
        let ranges = segment_ranges(&r);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].start, 0);
        // First \n included in left segment; second \n starts right segment.
        let blank_at = s.find("\n\n").unwrap();
        assert_eq!(ranges[0].end, blank_at + 1);
        assert_eq!(ranges[1].start, blank_at + 1);
        assert_eq!(ranges[1].end, s.len());
    }

    #[test]
    fn segment_ranges_three_paragraphs() {
        let s = "一\n\n二\n\n三";
        let r = rope(s);
        let ranges = segment_ranges(&r);
        assert_eq!(ranges.len(), 3, "{ranges:?}");
        // Coverage check: ranges concatenate to the full rope length.
        assert_eq!(ranges[0].start, 0);
        assert_eq!(ranges.last().unwrap().end, s.len());
        for w in ranges.windows(2) {
            assert_eq!(w[0].end, w[1].start, "no gap or overlap");
        }
    }

    #[test]
    fn parse_full_yields_one_tree_per_segment() {
        let doc = SegmentedDoc::new();
        let r = rope("段落1\n\n段落2\n\n段落3");
        doc.parse_full_rope(&r);
        assert_eq!(doc.segment_count(), 3);
        doc.with_segments(|segs| {
            for seg in segs {
                assert!(seg.tree.is_some(), "every non-empty segment has a tree");
            }
        });
    }

    #[test]
    fn apply_edit_inside_one_segment_does_not_touch_others() {
        let doc = SegmentedDoc::new();
        let mut r = rope("一段落\n\n二段落\n\n三段落");
        doc.parse_full_rope(&r);
        // tree-sitter `Tree::id` returns the underlying FFI pointer
        // value; identical id means no reparse happened, different id
        // means a fresh tree replaced the old one.
        let ids_before: Vec<usize> = doc.with_segments(|segs| {
            segs.iter()
                .map(|s| s.tree.as_ref().map_or(0, |t| t.root_node().id()))
                .collect()
        });

        // Insert a char inside the second segment.
        let mid = "一段落\n\n二段".len();
        let mut new_text = r.to_string();
        new_text.insert(mid, 'X');
        r = rope(&new_text);
        doc.apply_edit_rope(&r, input_edit(mid, mid, mid + 1));

        let ids_after: Vec<usize> = doc.with_segments(|segs| {
            segs.iter()
                .map(|s| s.tree.as_ref().map_or(0, |t| t.root_node().id()))
                .collect()
        });

        // Segment 0 untouched, segment 1 re-parsed, segment 2 untouched.
        assert_eq!(ids_before[0], ids_after[0], "seg 0 untouched");
        assert_ne!(ids_before[1], ids_after[1], "seg 1 re-parsed");
        assert_eq!(ids_before[2], ids_after[2], "seg 2 untouched");
    }

    #[test]
    fn apply_edit_shifts_byte_ranges_of_subsequent_segments() {
        let doc = SegmentedDoc::new();
        let mut r = rope("一\n\n二\n\n三");
        doc.parse_full_rope(&r);
        let ranges_before = doc.with_segments(|segs| {
            segs.iter()
                .map(|s| s.byte_range.clone())
                .collect::<Vec<_>>()
        });

        // Insert in segment 0.
        let mut new_text = r.to_string();
        new_text.insert(0, 'X');
        r = rope(&new_text);
        doc.apply_edit_rope(&r, input_edit(0, 0, 1));

        let ranges_after = doc.with_segments(|segs| {
            segs.iter()
                .map(|s| s.byte_range.clone())
                .collect::<Vec<_>>()
        });

        assert_eq!(ranges_after[0].len(), ranges_before[0].len() + 1);
        // Subsequent segments shifted by +1.
        for i in 1..ranges_before.len() {
            assert_eq!(ranges_after[i].start, ranges_before[i].start + 1);
            assert_eq!(ranges_after[i].end, ranges_before[i].end + 1);
        }
    }

    #[test]
    fn apply_edit_crossing_segments_falls_back_to_full_resegment() {
        let doc = SegmentedDoc::new();
        let mut r = rope("一\n\n二\n\n三");
        doc.parse_full_rope(&r);
        // Replace "\n\n二\n\n" with "" — the edit spans segment 0
        // through segment 2.
        let blank1 = "一".len();
        let blank2 = "一\n\n二\n\n".len();
        let mut new_text = r.to_string();
        new_text.replace_range(blank1..blank2, "");
        r = rope(&new_text);
        doc.apply_edit_rope(&r, input_edit(blank1, blank2, blank1));
        // Result is one segment: "一三".
        assert_eq!(doc.segment_count(), 1);
        doc.with_segments(|segs| {
            assert_eq!(segs[0].byte_range, 0..r.len_bytes());
        });
    }

    #[test]
    fn empty_doc_then_edit_initialises_segments() {
        let doc = SegmentedDoc::new();
        // Skip parse_full_rope; immediate apply_edit on a non-empty rope.
        let r = rope("hello");
        doc.apply_edit_rope(&r, input_edit(0, 0, 5));
        assert_eq!(doc.segment_count(), 1);
    }

    #[test]
    fn segments_concatenate_to_full_rope_after_edits() {
        let doc = SegmentedDoc::new();
        let mut r = rope("first\n\nsecond\n\nthird");
        doc.parse_full_rope(&r);

        // Insert + delete + insert series.
        let mut text = r.to_string();
        // Insert at 0
        text.insert_str(0, "PRE-");
        r = rope(&text);
        doc.apply_edit_rope(&r, input_edit(0, 0, "PRE-".len()));

        // Append at end
        let len = text.len();
        text.push_str("-END");
        r = rope(&text);
        doc.apply_edit_rope(&r, input_edit(len, len, len + "-END".len()));

        // Verify segment ranges still cover the full rope.
        let total = r.len_bytes();
        doc.with_segments(|segs| {
            assert_eq!(segs[0].byte_range.start, 0);
            assert_eq!(segs.last().unwrap().byte_range.end, total);
            for w in segs.windows(2) {
                assert_eq!(w[0].byte_range.end, w[1].byte_range.start);
            }
        });
    }
}
