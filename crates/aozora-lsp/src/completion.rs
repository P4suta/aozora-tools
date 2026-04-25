//! `textDocument/completion` handler — Phase 2.3 of the
//! editor-integration sprint, plus the Phase 2.5 half-width-bracket
//! input affordance.
//!
//! When the cursor sits in or just after either:
//!
//! - `［＃` — the canonical full-width slug-open digraph, or
//! - `[#`  — the half-width ASCII alternative typists reach for first,
//!
//! suggest every entry from the canonical [`aozora::SLUGS`] table.
//! Each suggestion carries:
//!
//! - `label`: the canonical body text the editor displays.
//! - `kind`: a [`CompletionItemKind`] picked from the slug's family.
//! - `detail` / `documentation`: short Japanese description from the
//!   slug entry's `doc` field.
//! - `text_edit`: a [`TextEdit`] that replaces the typed prefix-plus-
//!   body with the full canonical form `［＃canonical］` (or just
//!   `canonical` when the user already has the brackets). Half-width
//!   prefixes are auto-converted to full-width on accept — no
//!   separate "convert to ［＃" code action needed.
//! - `additional_text_edits`: when the slug is paired
//!   (`BlockContainerOpen` etc.) and the source does not already
//!   contain a matching close, append the close marker on a fresh
//!   line so the editor can land both in one accept.

use aozora::{SLUGS, SlugEntry, SlugFamily};
use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, InsertTextFormat,
    MarkupContent, MarkupKind, Position, Range, TextEdit,
};

use crate::position::byte_offset_to_position;

// We deliberately fire the catalogue from the very first byte of body
// — the editor's own fuzzy-match layer narrows from there. No minimum
// prefix length gate is applied (any "trigger after N keystrokes"
// behaviour belongs to the client side, not the server).

/// Recognised slug-open digraphs. Order matters only for `rfind`
/// disambiguation when both forms are present in `source[..cursor]` —
/// we pick whichever sits closer to the cursor (the latest opener).
const FULL_WIDTH_OPEN: &str = "［＃";
const HALF_WIDTH_OPEN: &str = "[#";

/// Resolved slug context for a cursor position.
struct SlugCtx {
    /// Byte offset where the opener (full- or half-width) starts.
    prefix_start: usize,
    /// Byte offset where the body starts (just after the opener).
    body_start: usize,
    /// Byte offset where the (existing) close `]` / `］` starts, or
    /// the cursor position when no close has been typed yet. The
    /// completion `text_edit` replaces the range
    /// `prefix_start..close_end` with the full canonical form.
    close_end: usize,
    /// True when the opener is `[#` (ASCII). On accept, the full-width
    /// `［＃...］` form is spliced in.
    half_width: bool,
}

/// Compute completion items at `position` in `source`. Returns an
/// empty Vec if the position is not in a slug context.
///
/// Tree-free: the slug-context detection is a small look-back scan
/// from the cursor (cap 256 bytes), so this handler is `O(window)`
/// regardless of document size. A future iteration that filters the
/// catalogue by enclosing container kind can re-introduce the
/// tree-sitter tree as a parameter.
#[must_use]
pub fn completion_at(source: &str, position: Position) -> Vec<CompletionItem> {
    let Some(byte_offset) = crate::position::position_to_byte_offset(source, position) else {
        return Vec::new();
    };
    let Some(ctx) = resolve_slug_context(source, byte_offset) else {
        return Vec::new();
    };

    // Return the full catalogue and let the LSP client's own
    // fuzzy-match layer filter — this is the norm for completion
    // providers (rust-analyzer, gopls, …) and avoids accidentally
    // hiding entries when the user types in latin transliteration
    // (`[#bouten` should still show 傍点).
    SLUGS
        .iter()
        .map(|entry| build_completion_item(source, entry, &ctx))
        .collect()
}

/// Find the latest slug opener (`［＃` or `[#`) before `cursor`,
/// classify whether the close has been typed, and bail out when the
/// cursor sits past an already-closed slug.
fn resolve_slug_context(source: &str, cursor: usize) -> Option<SlugCtx> {
    let prefix_full = source[..cursor].rfind(FULL_WIDTH_OPEN);
    let prefix_half = source[..cursor].rfind(HALF_WIDTH_OPEN);
    let (prefix_start, half_width, opener_len) = match (prefix_full, prefix_half) {
        (Some(f), Some(h)) if h > f => (h, true, HALF_WIDTH_OPEN.len()),
        (Some(f), _) => (f, false, FULL_WIDTH_OPEN.len()),
        (None, Some(h)) => (h, true, HALF_WIDTH_OPEN.len()),
        (None, None) => return None,
    };
    let body_start = prefix_start + opener_len;
    if cursor < body_start {
        return None;
    }
    let body_so_far = &source[body_start..cursor];
    // Bail when the user has already closed *this* slug between the
    // opener and the cursor (the prefix is not the *current* slug
    // context — the cursor is past it).
    if body_so_far.contains('］') || body_so_far.contains(']') {
        return None;
    }
    // Look forward for a matching close. We accept either ] or ］ as
    // a candidate close, stopping at the first newline (slug bodies
    // never span lines in well-formed Aozora).
    let close_end = source[cursor..]
        .char_indices()
        .find_map(|(rel, ch)| match ch {
            '］' => Some(cursor + rel + '］'.len_utf8()),
            ']' => Some(cursor + rel + ']'.len_utf8()),
            '\n' => Some(cursor),
            _ => None,
        })
        .unwrap_or(cursor);
    Some(SlugCtx {
        prefix_start,
        body_start,
        close_end,
        half_width,
    })
}

fn build_completion_item(source: &str, entry: &SlugEntry, ctx: &SlugCtx) -> CompletionItem {
    let detail = if ctx.half_width {
        format!("{}  (半角→全角)", entry.doc)
    } else {
        entry.doc.to_owned()
    };
    let mut item = CompletionItem {
        label: entry.canonical.to_owned(),
        kind: Some(family_to_kind(entry.family)),
        detail: Some(detail),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!(
                "**{family:?}** {accepts}\n\n{doc}",
                family = entry.family,
                accepts = if entry.accepts_param {
                    "(パラメータあり)"
                } else {
                    ""
                },
                doc = entry.doc,
            ),
        })),
        ..CompletionItem::default()
    };

    // Build the body text (snippet for parametric entries, plain
    // otherwise). The body text is the slug's canonical form sans
    // brackets.
    let (body_text, format) = if entry.accepts_param {
        (
            canonical_to_snippet(entry.canonical),
            InsertTextFormat::SNIPPET,
        )
    } else {
        (entry.canonical.to_owned(), InsertTextFormat::PLAIN_TEXT)
    };

    // Splice into the document. For half-width openers, also rewrite
    // the prefix to its full-width form so the source ends up
    // canonical regardless of which bracket the user typed first.
    let new_text = if ctx.half_width {
        // Replace `[#...maybe ]` with `［＃<canonical>］`.
        format!("［＃{body_text}］")
    } else if ctx.close_end > ctx.body_start
        && source[ctx.body_start..ctx.close_end].ends_with('］')
    {
        // Full-width opener with a full-width close already typed.
        // Replace just the body (between opener and close) with the
        // canonical form; leave the brackets in place.
        body_text
    } else {
        // Full-width opener but no close yet — emit the body and the
        // close so the user lands a balanced slug.
        format!("{body_text}］")
    };

    let edit_start = if ctx.half_width {
        ctx.prefix_start
    } else {
        ctx.body_start
    };
    let edit_end = ctx.close_end.max(edit_start);
    let range = Range::new(
        byte_offset_to_position(source, edit_start),
        byte_offset_to_position(source, edit_end),
    );
    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit { range, new_text }));
    item.insert_text_format = Some(format);

    // For BlockContainerOpen entries, append the matching close on a
    // fresh line so the user lands both halves in one accept. We
    // don't auto-insert when the partner is already present; a quick
    // forward scan covers the common case.
    if entry.family == SlugFamily::BlockContainerOpen
        && let Some(partner) = entry.partner
        && !forward_already_has(source, ctx.close_end, partner)
    {
        let pos = byte_offset_to_position(source, ctx.close_end);
        item.additional_text_edits = Some(vec![TextEdit {
            range: Range::new(pos, pos),
            // Two-line gap before the close so the body is visually
            // distinct. The editor's auto-format pass collapses extra
            // newlines if the user prefers tighter spacing.
            new_text: format!("\n\n［＃{partner}］"),
        }]);
    }
    item
}

/// Translate a canonical slug like `ここから{N}字下げ` into the LSP
/// snippet form `ここから${1:N}字下げ`. Plain text without `{...}`
/// pass-through unchanged. `{path}` becomes `${1:path}`.
fn canonical_to_snippet(canonical: &str) -> String {
    let mut out = String::with_capacity(canonical.len() + 4);
    let mut chars = canonical.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch == '{' {
            let rest = &canonical[i..];
            if let Some(close_rel) = rest.find('}') {
                let placeholder = &rest[1..close_rel];
                out.push_str("${1:");
                out.push_str(placeholder);
                out.push('}');
                while let Some(&(j, _)) = chars.peek() {
                    if j > i + close_rel {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }
        out.push(ch);
    }
    out
}

/// Quick forward scan: does `source[cursor..]` already contain
/// `partner` before EOF? Used to decide whether the
/// `BlockContainerOpen` completion should auto-insert the close.
fn forward_already_has(source: &str, cursor: usize, partner: &str) -> bool {
    source[cursor..].contains(partner)
}

fn family_to_kind(family: SlugFamily) -> CompletionItemKind {
    match family {
        SlugFamily::PageBreak | SlugFamily::Section => CompletionItemKind::EVENT,
        SlugFamily::BlockContainerOpen | SlugFamily::BlockContainerClose => {
            CompletionItemKind::CLASS
        }
        SlugFamily::LeafAlign => CompletionItemKind::ENUM_MEMBER,
        SlugFamily::Bouten => CompletionItemKind::FIELD,
        SlugFamily::Sashie => CompletionItemKind::FILE,
        SlugFamily::Keigakomi | SlugFamily::Warichu => CompletionItemKind::STRUCT,
        SlugFamily::TateChuYoko => CompletionItemKind::OPERATOR,
        SlugFamily::KaeritenSingle | SlugFamily::KaeritenCompound => CompletionItemKind::CONSTANT,
        // SlugFamily is `#[non_exhaustive]`; future families default
        // to TEXT until a kind is chosen.
        _ => CompletionItemKind::TEXT,
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn completion_inside_full_width_open_returns_full_catalogue() {
        let src = "前文［＃";
        
        
        let pos = byte_offset_to_position(src, src.len());
        let items = completion_at(src, pos);
        assert!(!items.is_empty(), "expected catalogue suggestions");
        assert!(
            items.iter().any(|i| i.label == "改ページ"),
            "expected 改ページ in completions",
        );
    }

    #[test]
    fn completion_inside_half_width_open_returns_full_catalogue() {
        let src = "[#";
        
        
        let pos = byte_offset_to_position(src, src.len());
        let items = completion_at(src, pos);
        assert!(!items.is_empty(), "[# must trigger completions too");
        // Half-width entries get a hint in the detail.
        assert!(
            items
                .iter()
                .all(|i| i.detail.as_deref().unwrap_or("").contains("半角→全角")),
            "half-width hint missing in detail",
        );
    }

    #[test]
    fn half_width_completion_replaces_prefix_with_full_width_form() {
        let src = "[#";
        
        
        let pos = byte_offset_to_position(src, src.len());
        let items = completion_at(src, pos);
        let entry = items
            .iter()
            .find(|i| i.label == "改ページ")
            .expect("改ページ in completions");
        let CompletionTextEdit::Edit(edit) = entry
            .text_edit
            .as_ref()
            .expect("text_edit present for half-width completion")
        else {
            panic!("expected Edit variant");
        };
        assert_eq!(edit.new_text, "［＃改ページ］");
    }

    #[test]
    fn half_width_completion_with_existing_close_replaces_through_close() {
        // `[#bouten]` — user typed both halves but in ASCII. Accepting
        // a completion should swap to `［＃改ページ］` (the matched
        // close is part of the replacement range).
        let src = "[#bouten]";
        
        
        // Cursor immediately after `bouten`.
        let pos = byte_offset_to_position(src, "[#bouten".len());
        let items = completion_at(src, pos);
        let entry = items
            .iter()
            .find(|i| i.label == "改ページ")
            .expect("改ページ in completions");
        let CompletionTextEdit::Edit(edit) = entry.text_edit.as_ref().unwrap() else {
            unreachable!()
        };
        assert_eq!(edit.new_text, "［＃改ページ］");
        // The text_edit's range must cover the full `[#bouten]`.
        assert_eq!(edit.range.start, byte_offset_to_position(src, 0));
        assert_eq!(edit.range.end, byte_offset_to_position(src, src.len()));
    }

    #[test]
    fn full_width_completion_keeps_brackets_in_place() {
        // User has `［＃改］` and the cursor inside the body. Accepting
        // a completion should replace just the body, leaving the
        // brackets untouched.
        let src = "［＃改］";
        
        
        let pos = byte_offset_to_position(src, "［＃".len() + "改".len());
        let items = completion_at(src, pos);
        let entry = items
            .iter()
            .find(|i| i.label == "改ページ")
            .expect("改ページ in completions");
        let CompletionTextEdit::Edit(edit) = entry.text_edit.as_ref().unwrap() else {
            unreachable!()
        };
        // body text only — no surrounding brackets.
        assert_eq!(edit.new_text, "改ページ");
    }

    #[test]
    fn completion_in_plain_text_returns_empty() {
        let src = "no annotation context";
        
        
        let pos = byte_offset_to_position(src, 4);
        assert!(completion_at(src, pos).is_empty());
    }

    #[test]
    fn completion_after_closed_full_width_bracket_returns_empty() {
        // The slug already closed before the cursor — no current open
        // context.
        let src = "前［＃改ページ］後";
        
        
        let pos = byte_offset_to_position(src, src.len());
        assert!(completion_at(src, pos).is_empty());
    }

    #[test]
    fn block_container_open_attaches_partner_close() {
        let src = "本文\n［＃";
        
        
        let pos = byte_offset_to_position(src, src.len());
        let items = completion_at(src, pos);
        let entry = items
            .iter()
            .find(|i| i.label == "ここから字下げ")
            .expect("ここから字下げ in suggestions");
        let edits = entry
            .additional_text_edits
            .as_ref()
            .expect("partner edit attached");
        assert_eq!(edits.len(), 1);
        assert!(
            edits[0].new_text.contains("ここで字下げ終わり"),
            "partner close text: {}",
            edits[0].new_text
        );
    }

    #[test]
    fn parametric_slug_emits_snippet_with_tabstop() {
        let src = "［＃";
        
        
        let pos = byte_offset_to_position(src, src.len());
        let items = completion_at(src, pos);
        let entry = items
            .iter()
            .find(|i| i.label == "{N}字下げ")
            .expect("{N}字下げ in suggestions");
        assert_eq!(entry.insert_text_format, Some(InsertTextFormat::SNIPPET));
        let CompletionTextEdit::Edit(edit) = entry.text_edit.as_ref().unwrap() else {
            unreachable!()
        };
        assert!(
            edit.new_text.contains("${1:"),
            "snippet missing tabstop: {:?}",
            edit.new_text
        );
    }
}
