//! Incremental tree-sitter state for the LSP backend.
//!
//! Each open document holds an [`IncrementalDoc`] alongside the
//! existing `SegmentCache`. The two parsers serve different masters:
//!
//! - **`SegmentCache` (semantic Rust parser)** — source of truth for
//!   diagnostics, HTML rendering, and formatting. Each invocation
//!   re-parses the whole document; called only when a heavy
//!   operation needs it (renderHtml, formatting, didChange's
//!   diagnostic publish).
//! - **`IncrementalDoc` (tree-sitter)** — keeps a tree synchronised
//!   with the buffer via incremental edits. Hover, inlay,
//!   codeAction, completion, and `linked_editing` all query this
//!   tree in microseconds, regardless of document size.
//!
//! ## Why two parsers
//!
//! The semantic parser is precise (gaiji resolution, kaeriten linking,
//! container nesting, diagnostic emission) but slow on big inputs.
//! Tree-sitter is structural-only (gaiji *spans*, ruby *spans*) but
//! fast and incremental. Letting the high-frequency LSP requests
//! (10–100 per second during editing) hit the fast parser keeps the
//! editor feel responsive even on 100 KB+ documents — the win the
//! 8:45 PM trace asked for.

use std::sync::Mutex;

use tree_sitter::{InputEdit, Parser, Point, Tree};

/// Per-document tree-sitter state.
///
/// `Mutex` because [`Parser`] is `!Sync` (it carries internal
/// stacks). Wrapped in a single struct so the LSP backend's
/// `DocState` doesn't have to synchronise the parser and tree
/// independently. Lock contention is negligible at LSP rates: each
/// edit takes microseconds on the incremental path.
pub struct IncrementalDoc {
    inner: Mutex<Inner>,
}

struct Inner {
    parser: Parser,
    tree: Option<Tree>,
}

impl std::fmt::Debug for IncrementalDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IncrementalDoc").finish_non_exhaustive()
    }
}

impl IncrementalDoc {
    /// Build a fresh per-document parser. The tree-sitter language
    /// is set once; subsequent calls reuse the parser.
    ///
    /// # Panics
    /// If the bundled `tree-sitter-aozora` grammar is incompatible
    /// with the linked tree-sitter runtime (ABI version mismatch).
    /// In practice this only fires if the build script regenerated
    /// the parser against a different runtime than the one resolved
    /// by Cargo.
    #[must_use]
    pub fn new() -> Self {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_aozora::LANGUAGE.into())
            .expect("tree-sitter-aozora language is compiled in");
        Self {
            inner: Mutex::new(Inner {
                parser,
                tree: None,
            }),
        }
    }

    /// Whole-document parse. Drops any existing tree. Used on
    /// `didOpen` and on full-replacement `didChange` events (where
    /// the LSP client decided to send the entire buffer instead of
    /// an incremental range).
    ///
    /// # Panics
    /// If the inner `Mutex` was poisoned by a previous panicking
    /// caller. With the current single-handler-per-doc usage this
    /// is unreachable in practice.
    pub fn parse_full(&self, text: &str) {
        let mut inner = self.inner.lock().expect("parser mutex");
        inner.tree = inner.parser.parse(text, None);
    }

    /// Apply an incremental edit. The caller has already mutated the
    /// underlying buffer; here we tell tree-sitter where the edit
    /// happened (in byte coordinates) and let it reparse only the
    /// affected sub-trees.
    ///
    /// The `Point` fields default to zero — tree-sitter accepts that
    /// when the column count is genuinely unused (we never query
    /// row/column after the edit). If a future feature needs
    /// position-accurate tree info, populate the `Point`s here.
    ///
    /// # Panics
    /// On a poisoned inner `Mutex` (see [`Self::parse_full`]).
    pub fn apply_edit(&self, new_text: &str, edit: InputEdit) {
        let mut inner = self.inner.lock().expect("parser mutex");
        // Take the existing tree out so the borrow checker lets us
        // pass `&self.parser` and `&Tree` simultaneously into
        // `parse`. The new tree replaces it.
        let mut prior = inner.tree.take();
        if let Some(tree) = prior.as_mut() {
            tree.edit(&edit);
        }
        inner.tree = inner.parser.parse(new_text, prior.as_ref());
    }

    /// Run a closure against the current tree. Returns `None` when
    /// no parse has been recorded yet (newly opened empty docs).
    ///
    /// The callback runs while the parser mutex is held — keep it
    /// short. For longer work (allocating Vecs, running multiple
    /// queries) consider cloning out the relevant nodes inside the
    /// closure and processing them after the lock is released.
    ///
    /// # Panics
    /// On a poisoned inner `Mutex` (see [`Self::parse_full`]).
    pub fn with_tree<R>(&self, f: impl FnOnce(&Tree) -> R) -> Option<R> {
        let inner = self.inner.lock().expect("parser mutex");
        inner.tree.as_ref().map(f)
    }
}

impl Default for IncrementalDoc {
    fn default() -> Self {
        Self::new()
    }
}

/// Translate an LSP-style "old text → new text" edit into a
/// tree-sitter [`InputEdit`]. The caller supplies byte offsets;
/// the `Point` fields are left at zero because the LSP backend's
/// tree consumers never query row/column positions on the TS tree
/// (everything is byte-driven via [`crate::position`]).
#[must_use]
pub fn input_edit(
    start_byte: usize,
    old_end_byte: usize,
    new_end_byte: usize,
) -> InputEdit {
    InputEdit {
        start_byte,
        old_end_byte,
        new_end_byte,
        start_position: Point::default(),
        old_end_position: Point::default(),
        new_end_position: Point::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter_aozora::kind;

    #[test]
    fn full_parse_then_query() {
        let doc = IncrementalDoc::new();
        doc.parse_full("｜青空《あおぞら》");
        let kind = doc
            .with_tree(|tree| tree.root_node().named_child(0).map(|n| n.kind().to_owned()))
            .flatten()
            .expect("tree exists");
        assert_eq!(kind, kind::EXPLICIT_RUBY);
    }

    #[test]
    fn incremental_edit_keeps_tree_in_sync() {
        let doc = IncrementalDoc::new();
        let initial = "｜青空《あおぞら》";
        doc.parse_full(initial);

        // Replace 「あおぞら」 with 「そら」 — same shape, shorter
        // reading. Tree-sitter must reuse the surrounding ruby
        // structure rather than re-parse from scratch.
        let reading_start = initial.find("あおぞら").unwrap();
        let new_text = "｜青空《そら》";
        let edit = input_edit(
            reading_start,
            reading_start + "あおぞら".len(),
            reading_start + "そら".len(),
        );
        doc.apply_edit(new_text, edit);

        let kind = doc
            .with_tree(|tree| tree.root_node().named_child(0).map(|n| n.kind().to_owned()))
            .flatten()
            .expect("tree exists");
        assert_eq!(kind, kind::EXPLICIT_RUBY);
    }

    #[test]
    fn empty_doc_yields_none_for_with_tree_until_parse() {
        let doc = IncrementalDoc::new();
        let result = doc.with_tree(|_| 42);
        assert_eq!(result, None);
    }

    #[test]
    fn full_replace_after_initial_parse_drops_old_tree() {
        let doc = IncrementalDoc::new();
        doc.parse_full("plain text");
        doc.parse_full("｜青空《あおぞら》");
        let kind = doc
            .with_tree(|tree| tree.root_node().named_child(0).map(|n| n.kind().to_owned()))
            .flatten();
        assert_eq!(kind.as_deref(), Some(kind::EXPLICIT_RUBY));
    }
}
