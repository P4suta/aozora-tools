//! `textDocument/documentSymbol` — outline view from heading
//! markers in aozora notation.
//!
//! Aozora has three heading levels declared via slugs:
//! `大見出し` / `中見出し` / `小見出し`. The slug body either
//! references a forward quote (`「序章」は大見出し`) or precedes the
//! line text directly (`［＃大見出し］序章`). We accept both shapes.
//!
//! ## Output shape
//!
//! Returns a flat list of `DocumentSymbol`s in source order. Editors
//! that present a tree view will show 大 → 中 → 小 nesting because we
//! place each child under the most recent open heading of strictly
//! lower level.

use tower_lsp::lsp_types::{DocumentSymbol, Range, SymbolKind};

use crate::line_index::LineIndex;

/// Compute every heading symbol in `source`, nested by level.
#[must_use]
pub fn document_symbols(source: &str, line_index: &LineIndex) -> Vec<DocumentSymbol> {
    let mut flat: Vec<(HeadingLevel, DocumentSymbol)> = Vec::new();
    for (line_idx, line) in source.lines().enumerate() {
        let Some(level) = heading_level_in(line) else {
            continue;
        };
        let line_idx = u32::try_from(line_idx).unwrap_or(u32::MAX);
        // Compute byte range of this line by re-locating it via
        // line_index: start = line_index.line_to_byte; end = next
        // line's byte or source.len().
        let title = extract_title(line, source, line_idx, line_index);
        let symbol = build_symbol(line_idx, level, title);
        flat.push((level, symbol));
    }
    nest_by_level(flat)
}

/// Detect whether `line` declares a heading and at what level.
fn heading_level_in(line: &str) -> Option<HeadingLevel> {
    if !line.contains("見出し") {
        return None;
    }
    if line.contains("大見出し") {
        Some(HeadingLevel::Major)
    } else if line.contains("中見出し") {
        Some(HeadingLevel::Middle)
    } else if line.contains("小見出し") {
        Some(HeadingLevel::Minor)
    } else {
        // `見出し` without a 大/中/小 prefix — accept as Middle.
        // Real aozora docs almost always specify a level, so this is
        // a defensive default.
        Some(HeadingLevel::Middle)
    }
}

/// Best-effort title extraction from a heading line.
///
/// Three shapes:
/// 1. `「序章」は大見出し` — title is the quoted text.
/// 2. `［＃大見出し］序章［＃大見出し終わり］` — title is the text
///    after the opening slug.
/// 3. `［＃大見出し］序章` — title is whatever follows the slug on
///    the same line, until end-of-line or next slug.
fn extract_title(line: &str, _source: &str, _line_idx: u32, _line_index: &LineIndex) -> String {
    // Shape 1: quoted title
    if let Some(quote_start) = line.find('「') {
        let after = &line[quote_start + '「'.len_utf8()..];
        if let Some(quote_end) = after.find('」') {
            return after[..quote_end].to_owned();
        }
    }
    // Shape 2/3: text after closing ］
    if let Some(close_idx) = line.find('］') {
        let after = &line[close_idx + '］'.len_utf8()..];
        // Trim any trailing closing slug like `［＃大見出し終わり］`.
        let trimmed = after.find('［').map_or(after, |stop| &after[..stop]).trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    "(無題)".to_owned()
}

fn build_symbol(line_idx: u32, level: HeadingLevel, title: String) -> DocumentSymbol {
    let line_range = Range::new(
        tower_lsp::lsp_types::Position::new(line_idx, 0),
        tower_lsp::lsp_types::Position::new(line_idx, u32::MAX),
    );
    #[allow(
        deprecated,
        reason = "DocumentSymbol::deprecated is required by the LSP type"
    )]
    DocumentSymbol {
        name: title,
        detail: Some(level.as_str().to_owned()),
        kind: level.symbol_kind(),
        tags: None,
        deprecated: None,
        range: line_range,
        selection_range: line_range,
        children: Some(Vec::new()),
    }
}

/// Re-fold the flat list into a tree by `HeadingLevel` strict
/// containment: a Minor under the most recent Middle, a Middle
/// under the most recent Major, a Major at the top level.
fn nest_by_level(flat: Vec<(HeadingLevel, DocumentSymbol)>) -> Vec<DocumentSymbol> {
    // Build via index manipulation on a single growing root vec.
    // For each new symbol, walk down the rightmost path until we
    // find a parent of strictly lower level (smaller order). Push
    // there; otherwise push at root.
    let mut root: Vec<DocumentSymbol> = Vec::new();
    let mut stack: Vec<(HeadingLevel, Vec<usize>)> = Vec::new();
    for (level, sym) in flat {
        // Pop the stack until the top is strictly higher than `level`.
        while stack.last().is_some_and(|(top_lv, _)| *top_lv >= level) {
            stack.pop();
        }
        let path = stack.last().map(|(_, p)| p.clone()).unwrap_or_default();
        let parent_children = navigate_mut(&mut root, &path);
        let new_idx = parent_children.len();
        parent_children.push(sym);
        let mut new_path = path;
        new_path.push(new_idx);
        stack.push((level, new_path));
    }
    root
}

fn navigate_mut<'a>(
    root: &'a mut Vec<DocumentSymbol>,
    path: &[usize],
) -> &'a mut Vec<DocumentSymbol> {
    if path.is_empty() {
        return root;
    }
    let mut cur: &mut Vec<DocumentSymbol> = root;
    for (i, &idx) in path.iter().enumerate() {
        let node = &mut cur[idx];
        if i + 1 == path.len() {
            return node.children.get_or_insert_with(Vec::new);
        }
        cur = node.children.get_or_insert_with(Vec::new);
    }
    cur
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum HeadingLevel {
    Major = 0,  // 大見出し — outermost
    Middle = 1, // 中見出し
    Minor = 2,  // 小見出し
}

impl HeadingLevel {
    fn as_str(self) -> &'static str {
        match self {
            HeadingLevel::Major => "大見出し",
            HeadingLevel::Middle => "中見出し",
            HeadingLevel::Minor => "小見出し",
        }
    }

    fn symbol_kind(self) -> SymbolKind {
        match self {
            HeadingLevel::Major => SymbolKind::CLASS,
            HeadingLevel::Middle => SymbolKind::NAMESPACE,
            HeadingLevel::Minor => SymbolKind::FUNCTION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syms(src: &str) -> Vec<DocumentSymbol> {
        let idx = LineIndex::new(src);
        document_symbols(src, &idx)
    }

    #[test]
    fn empty_source_yields_no_symbols() {
        assert!(syms("").is_empty());
    }

    #[test]
    fn single_major_heading_quoted() {
        let src = "［＃「序章」は大見出し］\n本文\n";
        let s = syms(src);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "序章");
        assert_eq!(s[0].kind, SymbolKind::CLASS);
        assert_eq!(s[0].detail.as_deref(), Some("大見出し"));
    }

    #[test]
    fn single_middle_heading_inline_text() {
        let src = "［＃中見出し］第一節［＃中見出し終わり］\n";
        let s = syms(src);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].name, "第一節");
        assert_eq!(s[0].kind, SymbolKind::NAMESPACE);
    }

    #[test]
    fn nested_middle_under_major() {
        let src = "［＃「序章」は大見出し］\n本文\n［＃「第一節」は中見出し］\n本文2\n";
        let tree = syms(src);
        assert_eq!(tree.len(), 1);
        let major = &tree[0];
        assert_eq!(major.name, "序章");
        let children = major.children.as_ref().expect("children");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "第一節");
        assert_eq!(children[0].kind, SymbolKind::NAMESPACE);
    }

    #[test]
    fn nested_minor_under_middle_under_major() {
        let src = "\
［＃「巻」は大見出し］\n\
［＃「章」は中見出し］\n\
［＃「節」は小見出し］\n\
本文\n";
        let tree = syms(src);
        assert_eq!(tree.len(), 1);
        let major = &tree[0];
        let middle = &major.children.as_ref().unwrap()[0];
        let minor = &middle.children.as_ref().unwrap()[0];
        assert_eq!(major.name, "巻");
        assert_eq!(middle.name, "章");
        assert_eq!(minor.name, "節");
        assert_eq!(minor.kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn sibling_majors_at_root() {
        let src = "［＃「一」は大見出し］\n本\n［＃「二」は大見出し］\n本2\n";
        let tree = syms(src);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].name, "一");
        assert_eq!(tree[1].name, "二");
    }

    #[test]
    fn deeper_then_shallower_pops_stack() {
        let src = "\
［＃「巻一」は大見出し］\n\
［＃「節一」は中見出し］\n\
［＃「巻二」は大見出し］\n\
［＃「節二」は中見出し］\n";
        let tree = syms(src);
        assert_eq!(tree.len(), 2);
        // Each major has exactly its own middle as a child.
        assert_eq!(tree[0].children.as_ref().unwrap().len(), 1);
        assert_eq!(tree[0].children.as_ref().unwrap()[0].name, "節一");
        assert_eq!(tree[1].children.as_ref().unwrap().len(), 1);
        assert_eq!(tree[1].children.as_ref().unwrap()[0].name, "節二");
    }
}
