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

use std::collections::BTreeMap;
use std::sync::Arc;

use tree_sitter::{InputEdit, Node, Tree};
use tree_sitter_aozora::kind;

/// One `※［＃description、mencode］` occurrence.
///
/// `description` and `mencode` are `Arc<str>` rather than `String`
/// so the incremental snapshot rebuild can reuse them across
/// snapshot generations via pointer bump — the body of a gaiji
/// span doesn't change unless its bytes are inside a tree-sitter
/// `changed_range`. Carry-forward of a 50 k-span document then
/// costs `O(spans)` atomic increments instead of `O(spans)`
/// `String` allocations.
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
    pub description: Arc<str>,
    /// `第3水準1-85-54` / `U+1234` / similar — `None` if the source
    /// omitted the mencode tail.
    pub mencode: Option<Arc<str>>,
}

/// Walk `tree` once and extract every gaiji span. Output is sorted
/// by `start_byte` (the tree visit is in source order).
///
/// Iterative single-cursor walk via `TreeCursor::goto_first_child` /
/// `goto_next_sibling` / `goto_parent`. The tree carries ~100 k
/// nodes on a 6 MB document; the earlier recursive `node.walk()`
/// version allocated a fresh `TreeCursor` *per non-gaiji node* and
/// recursed Rust stack frames in lock-step — both lit up the
/// allocator hotpath in samply.
///
/// We also tried the tree-sitter `Query` API (`(gaiji) @g`); it ran
/// ~5× slower (71 ms → 330 ms) because the `QueryCursor`'s general
/// pattern-matching automaton has more overhead per visited node
/// than a single-kind dispatch. Iterative cursor wins for
/// "find every node of one kind, no predicates" extraction.
#[must_use]
pub fn extract_gaiji_spans(tree: &Tree, source: &str) -> Arc<[Arc<GaijiSpan>]> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut spans = Vec::new();
    'walk: loop {
        let node = cursor.node();
        if node.kind() == kind::GAIJI {
            if let Some(span) = build_span(node, source) {
                spans.push(Arc::new(span));
            }
            // gaiji nodes are leaves for this walk — skip descent.
        } else if cursor.goto_first_child() {
            continue;
        }
        // Either a leaf (gaiji or terminal) or a node with no
        // children. Walk laterally; pop up while there's no sibling.
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
    spans.into()
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
fn parse_body(body: &str) -> (Arc<str>, Option<Arc<str>>) {
    let (description, rest) = body.find('、').map_or((body, None), |i| {
        let head = body[..i].trim();
        let tail = body[i + '、'.len_utf8()..].trim();
        (head, Some(tail))
    });
    let description = description.trim().trim_matches(|c| c == '「' || c == '」');
    let mencode = rest.map(str::trim).filter(|s| !s.is_empty()).map(Arc::from);
    (Arc::from(description), mencode)
}

/// Filter `spans` to those whose `start_byte` lies in
/// `[start_byte, end_byte)`. Uses binary search on the sorted
/// `start_byte` field so the per-request cost stays
/// `O(log spans + matches)`.
#[must_use]
pub fn spans_in_byte_range(
    spans: &[Arc<GaijiSpan>],
    start_byte: usize,
    end_byte: usize,
) -> &[Arc<GaijiSpan>] {
    let start = u32::try_from(start_byte).unwrap_or(u32::MAX);
    let end = u32::try_from(end_byte).unwrap_or(u32::MAX);
    // First span whose end exceeds the start of the requested range.
    let lo = spans.partition_point(|s| s.end_byte <= start);
    // First span whose start is past the end of the requested range.
    let hi = spans.partition_point(|s| s.start_byte < end);
    &spans[lo..hi.max(lo)]
}

/// Build the next snapshot's gaiji span store **incrementally** from
/// the prior snapshot.
///
/// Algorithm (overall cost: `O(log n + k)` where `k` is the number of
/// spans within the changed byte ranges):
///
/// 1. `old_tree.changed_ranges(new_tree)` — tree-sitter returns the
///    byte ranges (in `new_text` coordinates) where the new tree's
///    structure differs from the old. These are the regions the
///    parser had to re-parse.
/// 2. For every span in `old_spans`, apply the cumulative `edits`
///    delta to translate its `(start_byte, end_byte)` into new-text
///    coordinates. If the new range intersects any changed range, or
///    if any edit splits the span itself, drop the span — it will be
///    re-extracted from the new tree. Otherwise, carry it forward
///    with the shifted offsets.
/// 3. For each changed range, walk the new tree restricted to that
///    range and extract any gaiji nodes. Re-walk reuses the same
///    iterative `TreeCursor` pattern as the cold-start path.
///
/// The savings vs full re-walk are dramatic for typical edits
/// (cursor in the middle of the doc): only the small local region
/// needs a fresh walk, the surrounding ~24 k spans pass through
/// almost free. For pathological edits (insert at offset 0, every
/// byte shifts), `changed_ranges` covers the whole document and
/// the algorithm degenerates to full re-walk — same cost as the
/// cold path, no regression.
#[must_use]
pub fn extract_gaiji_spans_incremental(
    old_tree: &Tree,
    new_tree: &Tree,
    old_spans: &BTreeMap<u32, Arc<GaijiSpan>>,
    edits: &[InputEdit],
    new_text: &str,
) -> BTreeMap<u32, Arc<GaijiSpan>> {
    // Tree-sitter `changed_ranges` returns byte ranges (NEW tree
    // coordinates) where structure differs. On worst-case edits
    // (insert at offset 0 → every token shifts) the iterator can
    // emit thousands of small ranges; we sort + merge them once
    // here so the per-span check below stays `O(log m)` and the
    // walker stays `O(visits * log m)` instead of degenerating to
    // `O(n * m)`.
    let mut raw: Vec<(u32, u32)> = old_tree
        .changed_ranges(new_tree)
        .filter_map(|r| {
            let s = u32::try_from(r.start_byte).ok()?;
            let e = u32::try_from(r.end_byte).ok()?;
            if s < e { Some((s, e)) } else { None }
        })
        .collect();
    raw.sort_unstable_by_key(|&(s, _)| s);
    let changed = merge_sorted_ranges(raw);

    let mut out = BTreeMap::new();

    // 1. Carry forward old spans that don't intersect any merged
    //    changed range (in NEW coordinates). Each span goes through
    //    `shift_through_edits` then a `O(log m)` binary-search check.
    for span in old_spans.values() {
        let Some((new_start, new_end)) = shift_through_edits(span.start_byte, span.end_byte, edits)
        else {
            // Edit clipped through the span; will be re-extracted.
            continue;
        };
        if intersects_sorted(new_start, new_end, &changed) {
            continue;
        }
        // Two cheap carry-forward shapes:
        //   - Unchanged byte offsets → reuse the entire `Arc<GaijiSpan>`
        //     (single atomic increment, zero allocations).
        //   - Shifted byte offsets → allocate a fresh `Arc<GaijiSpan>`
        //     but pointer-bump the description / mencode `Arc<str>`s.
        //     Avoids the per-span `String` clone the prior shape paid.
        let carried = if span.start_byte == new_start && span.end_byte == new_end {
            Arc::clone(span)
        } else {
            Arc::new(GaijiSpan {
                start_byte: new_start,
                end_byte: new_end,
                description: Arc::clone(&span.description),
                mencode: span.mencode.clone(),
            })
        };
        out.insert(new_start, carried);
    }

    // 2. Single iterative tree walk that prunes against the merged
    //    range set — visits only subtrees that intersect at least
    //    one changed range. Re-extracts any gaiji nodes there.
    walk_against_ranges(new_tree.root_node(), new_text, &changed, &mut out);

    out
}

/// Merge a sorted list of `(start, end)` ranges into a non-overlapping
/// set. Adjacent ranges (`end == next.start`) are coalesced.
fn merge_sorted_ranges(sorted: Vec<(u32, u32)>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(sorted.len());
    for (s, e) in sorted {
        match out.last_mut() {
            Some(last) if last.1 >= s => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Binary-search whether `[start, end)` intersects any of the
/// sorted, non-overlapping `ranges`. `O(log n)`.
fn intersects_sorted(start: u32, end: u32, ranges: &[(u32, u32)]) -> bool {
    // First range whose end > start.
    let i = ranges.partition_point(|&(_, e)| e <= start);
    i < ranges.len() && ranges[i].0 < end
}

/// Translate `(start, end)` byte offsets through the cumulative
/// `edits` list. Returns `None` if any edit modifies bytes inside the
/// span itself (the span needs re-extraction in that case).
///
/// Each `InputEdit` shifts subsequent bytes by
/// `new_end_byte - old_end_byte`. Applied in order.
fn shift_through_edits(start: u32, end: u32, edits: &[InputEdit]) -> Option<(u32, u32)> {
    let mut start = i64::from(start);
    let mut end = i64::from(end);
    for edit in edits {
        // Source byte offsets carried by tree-sitter `InputEdit` are
        // `usize`. Documents we care about cap well under `i64::MAX`,
        // so these casts can't overflow on real inputs.
        let edit_old_start = i64::try_from(edit.start_byte).unwrap_or(i64::MAX);
        let edit_old_end = i64::try_from(edit.old_end_byte).unwrap_or(i64::MAX);
        let edit_new_end = i64::try_from(edit.new_end_byte).unwrap_or(i64::MAX);
        let delta = edit_new_end - edit_old_end;
        if start >= edit_old_end {
            // Span entirely after this edit — shift by delta.
            start += delta;
            end += delta;
        } else if end <= edit_old_start {
            // Span entirely before this edit — no shift.
        } else {
            // Span overlaps the edit region — needs re-extraction.
            return None;
        }
    }
    if start < 0 || end < 0 {
        return None;
    }
    Some((
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    ))
}

/// Single iterative tree walk that descends only into subtrees
/// intersecting one of the sorted, non-overlapping `ranges`. Visits
/// every gaiji node within those ranges and inserts its span into
/// `out`. Per-node intersection check is `O(log ranges)` via
/// [`intersects_sorted`].
fn walk_against_ranges(
    root: Node<'_>,
    source: &str,
    ranges: &[(u32, u32)],
    out: &mut BTreeMap<u32, Arc<GaijiSpan>>,
) {
    if ranges.is_empty() {
        return;
    }
    let mut cursor = root.walk();
    'walk: loop {
        let node = cursor.node();
        let start = u32::try_from(node.start_byte()).unwrap_or(u32::MAX);
        let end = u32::try_from(node.end_byte()).unwrap_or(u32::MAX);
        if !intersects_sorted(start, end, ranges) {
            // Outside every changed range — skip without descending.
        } else if node.kind() == kind::GAIJI {
            if let Some(span) = build_span(node, source) {
                out.insert(span.start_byte, Arc::new(span));
            }
            // gaiji is a leaf — fall through to lateral move.
        } else if cursor.goto_first_child() {
            continue;
        }
        while !cursor.goto_next_sibling() {
            if !cursor.goto_parent() {
                break 'walk;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Parser, Point};

    fn parse(src: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .unwrap();
        parser.parse(src, None).unwrap()
    }

    fn cold_btree(src: &str) -> BTreeMap<u32, Arc<GaijiSpan>> {
        let tree = parse(src);
        extract_gaiji_spans(&tree, src)
            .iter()
            .map(|s| (s.start_byte, Arc::clone(s)))
            .collect()
    }

    /// Apply one `InputEdit` to `(text, tree)` and return `(new_text, new_tree, edit)`.
    fn edit_once(
        old_text: &str,
        old_tree: &Tree,
        start_byte: usize,
        old_end_byte: usize,
        new_substr: &str,
    ) -> (String, Tree, InputEdit) {
        let new_text = format!(
            "{}{}{}",
            &old_text[..start_byte],
            new_substr,
            &old_text[old_end_byte..]
        );
        let new_end_byte = start_byte + new_substr.len();
        let edit = InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position: Point::default(),
            old_end_position: Point::default(),
            new_end_position: Point::default(),
        };
        let mut edited = old_tree.clone();
        edited.edit(&edit);
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .unwrap();
        let new_tree = parser.parse(&new_text, Some(&edited)).unwrap();
        (new_text, new_tree, edit)
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
        assert_eq!(&*span.description, "desc");
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
        assert_eq!(&*spans[0].description, "a");
        assert_eq!(&*spans[1].description, "b");
    }

    #[test]
    fn description_only_form_yields_none_mencode() {
        let src = "※［＃「desc-only」］";
        let tree = parse(src);
        let spans = extract_gaiji_spans(&tree, src);
        assert_eq!(spans.len(), 1);
        assert_eq!(&*spans[0].description, "desc-only");
        assert!(spans[0].mencode.is_none());
    }

    /// Editing in the middle of a doc, away from any gaiji nodes,
    /// must produce the same span set as a full re-walk against the
    /// new tree. The carry-forward path with byte-shift should
    /// preserve every existing span.
    #[test]
    fn incremental_matches_full_after_isolated_text_edit() {
        let src = "※［＃「a」、X］\nplain plain plain\n※［＃「b」、Y］";
        let old_tree = parse(src);
        let old_spans = cold_btree(src);
        // Insert "ZZ" inside the plain region (between the two gaijis).
        let plain_offset = src.find("plain").unwrap();
        let (new_text, new_tree, edit) =
            edit_once(src, &old_tree, plain_offset, plain_offset, "ZZ");
        let inc =
            extract_gaiji_spans_incremental(&old_tree, &new_tree, &old_spans, &[edit], &new_text);
        let cold = cold_btree(&new_text);
        assert_eq!(inc, cold, "incremental result must equal cold rebuild");
    }

    /// Editing the description of an existing gaiji must
    /// re-extract its span (with the new description) — the
    /// overlapping-edit guard must drop the old span and the
    /// changed-range walk must surface the new one.
    #[test]
    fn incremental_re_extracts_changed_gaiji() {
        let src = "※［＃「old」、X］";
        let old_tree = parse(src);
        let old_spans = cold_btree(src);
        // Replace "old" with "renamed".
        let r_start = src.find("old").unwrap();
        let (new_text, new_tree, edit) =
            edit_once(src, &old_tree, r_start, r_start + "old".len(), "renamed");
        let inc =
            extract_gaiji_spans_incremental(&old_tree, &new_tree, &old_spans, &[edit], &new_text);
        let cold = cold_btree(&new_text);
        assert_eq!(inc, cold);
        // Sanity: the new doc's only span carries the new description.
        let only = inc.values().next().expect("one span");
        assert_eq!(&*only.description, "renamed");
    }

    /// Inserting a brand-new gaiji must add it to the `BTreeMap`
    /// without disturbing the existing spans' offsets (modulo the
    /// edit shift).
    #[test]
    fn incremental_picks_up_newly_inserted_gaiji() {
        let src = "before\nafter";
        let old_tree = parse(src);
        let old_spans = cold_btree(src);
        let insert_at = src.find("\nafter").unwrap();
        let new_chunk = "\n※［＃「new」、X］";
        let (new_text, new_tree, edit) = edit_once(src, &old_tree, insert_at, insert_at, new_chunk);
        let inc =
            extract_gaiji_spans_incremental(&old_tree, &new_tree, &old_spans, &[edit], &new_text);
        let cold = cold_btree(&new_text);
        assert_eq!(inc, cold);
        assert_eq!(inc.len(), 1);
    }

    /// Worst case: insert at byte 0 shifts every span. The
    /// incremental algorithm must still yield the cold result —
    /// either via shifting carry-forward or via a full re-walk
    /// fallback.
    #[test]
    fn incremental_handles_offset_zero_insert() {
        let src = "※［＃「a」、X］後";
        let old_tree = parse(src);
        let old_spans = cold_btree(src);
        let (new_text, new_tree, edit) = edit_once(src, &old_tree, 0, 0, "Z");
        let inc =
            extract_gaiji_spans_incremental(&old_tree, &new_tree, &old_spans, &[edit], &new_text);
        let cold = cold_btree(&new_text);
        assert_eq!(inc, cold);
    }

    /// Removing every gaiji should yield an empty result. The
    /// drop-on-overlap guard must catch deletions through the spans.
    #[test]
    fn incremental_handles_deletion_of_only_gaiji() {
        let src = "※［＃「a」、X］";
        let old_tree = parse(src);
        let old_spans = cold_btree(src);
        let (new_text, new_tree, edit) = edit_once(src, &old_tree, 0, src.len(), "");
        let inc =
            extract_gaiji_spans_incremental(&old_tree, &new_tree, &old_spans, &[edit], &new_text);
        let cold = cold_btree(&new_text);
        assert_eq!(inc, cold);
        assert!(inc.is_empty());
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
        assert_eq!(&*inside[0].description, "b");
    }
}
