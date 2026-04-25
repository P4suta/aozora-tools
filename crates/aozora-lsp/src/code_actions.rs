//! `textDocument/codeAction` handler — Phase 2.5 (wrap selection in
//! delimiter pair).
//!
//! When the user has a non-empty selection in an aozora document, the
//! editor (right-click → Refactor, or Ctrl+. lightbulb) shows a menu
//! of wrap actions:
//!
//! - 《 ... 》 (ruby reading)
//! - 《《 ... 》》 (double ruby / bouten)
//! - 「 ... 」 (quoted literal)
//! - 〔 ... 〕 (accent decomposition)
//! - ［＃ ... ］ (annotation body)
//! - ｜SEL《》 (convert selection to ruby base — cursor lands inside the reading)
//! - SEL［＃「SEL」に傍点］ (forward-reference bouten)
//!
//! Each action is a [`CodeAction`] carrying a [`WorkspaceEdit`] that
//! splices the open/close around `selection`. The 縦中横 / 傍点
//! forward-reference variants additionally insert the
//! `［＃「TARGET」…］` directive after the selection, with `TARGET`
//! pre-filled to the selected text.

use tower_lsp::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Diagnostic, Range, TextEdit, Url,
    WorkspaceEdit,
};

use crate::diagnostics::{DiagnosticPayload, SerializablePairKind};
use crate::line_index::LineIndex;

/// Compute every wrap-selection [`CodeAction`] applicable to
/// `selection` in `source`. Returns an empty vec when the selection
/// is empty or unresolvable.
#[must_use]
pub fn wrap_selection_actions(
    source: &str,
    line_index: &LineIndex,
    uri: &Url,
    selection: Range,
) -> Vec<CodeActionOrCommand> {
    let Some(start) = line_index.byte_offset(source, selection.start) else {
        return Vec::new();
    };
    let Some(end) = line_index.byte_offset(source, selection.end) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    let selected = &source[start..end];

    let mut actions: Vec<CodeActionOrCommand> = Vec::new();
    actions.extend([
        wrap_pair(uri, selection, "《", "》", "ルビ読みで囲む 《...》"),
        wrap_pair(uri, selection, "《《", "》》", "二重ルビで囲む 《《...》》"),
        wrap_pair(uri, selection, "「", "」", "鉤括弧で囲む 「...」"),
        wrap_pair(uri, selection, "〔", "〕", "亀甲括弧で囲む 〔...〕"),
        wrap_pair(uri, selection, "［＃", "］", "注記で囲む ［＃...］"),
        ruby_base_action(uri, selection, selected),
        forward_bouten_action(uri, selection, selected),
    ]);
    actions
}

/// Build a single open/close wrap [`CodeAction`].
fn wrap_pair(
    uri: &Url,
    selection: Range,
    open: &str,
    close: &str,
    title: &str,
) -> CodeActionOrCommand {
    let edits = vec![
        TextEdit {
            range: Range::new(selection.start, selection.start),
            new_text: open.to_owned(),
        },
        TextEdit {
            range: Range::new(selection.end, selection.end),
            new_text: close.to_owned(),
        },
    ];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: title.to_owned(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        ..CodeAction::default()
    })
}

/// Convert the selection into an explicit-delimiter ruby base:
/// `SELECTION` → `｜SELECTION《》`. The reading slot is intentionally
/// empty; the editor lands the cursor between `《》` after applying.
fn ruby_base_action(uri: &Url, selection: Range, _selected: &str) -> CodeActionOrCommand {
    let edits = vec![
        TextEdit {
            range: Range::new(selection.start, selection.start),
            new_text: "｜".to_owned(),
        },
        TextEdit {
            range: Range::new(selection.end, selection.end),
            new_text: "《》".to_owned(),
        },
    ];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: "ルビベース化 ｜SEL《》".to_owned(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        ..CodeAction::default()
    })
}

/// Convert the LSP-supplied `params.context.diagnostics` into a
/// quick-fix [`CodeAction`] list. Each diagnostic carries a JSON
/// `data` payload (set by [`crate::diagnostics::describe`]) describing
/// what kind of fix is appropriate; this function decodes the
/// payload and emits a concrete [`WorkspaceEdit`].
///
/// Returns an empty `Vec` when no diagnostic in the request range
/// has a known fix shape.
#[must_use]
pub fn quick_fix_actions(uri: &Url, diagnostics: &[Diagnostic]) -> Vec<CodeActionOrCommand> {
    diagnostics
        .iter()
        .filter_map(|diag| {
            let payload = diag
                .data
                .as_ref()
                .and_then(|v| serde_json::from_value::<DiagnosticPayload>(v.clone()).ok())?;
            build_quick_fix(uri, diag, payload)
        })
        .collect()
}

fn build_quick_fix(
    uri: &Url,
    diag: &Diagnostic,
    payload: DiagnosticPayload,
) -> Option<CodeActionOrCommand> {
    match payload {
        DiagnosticPayload::UnclosedBracket {
            pair_kind,
            expected_close,
        } => Some(insert_close_action(uri, diag, pair_kind, &expected_close)),
        DiagnosticPayload::UnmatchedClose { pair_kind } => {
            Some(delete_unmatched_close_action(uri, diag, pair_kind))
        }
        DiagnosticPayload::SourceContainsPua { codepoint } => {
            Some(delete_pua_action(uri, diag, codepoint))
        }
        // ResidualAnnotationMarker → no automatic fix (the user must
        // choose which keyword they meant); the diagnostic's verbose
        // message lists the manual recovery steps.
        DiagnosticPayload::ResidualAnnotationMarker => None,
    }
}

fn insert_close_action(
    uri: &Url,
    diag: &Diagnostic,
    pair_kind: SerializablePairKind,
    close: &str,
) -> CodeActionOrCommand {
    // Insert the close at the end of the diagnostic's range — that
    // sits just past the unclosed open delimiter, which is the most
    // ergonomic landing spot for the auto-fix. The user can move it
    // afterward if they meant for the body to extend further.
    let edits = vec![TextEdit {
        range: Range::new(diag.range.end, diag.range.end),
        new_text: close.to_owned(),
    }];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("`{close}` を補って閉じる ({} ペア)", pair_kind.open_str()),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })
}

fn delete_unmatched_close_action(
    uri: &Url,
    diag: &Diagnostic,
    pair_kind: SerializablePairKind,
) -> CodeActionOrCommand {
    let close = pair_kind.close_str();
    // Replace the diagnostic span (the stray close) with empty text.
    let edits = vec![TextEdit {
        range: diag.range,
        new_text: String::new(),
    }];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("対応のない `{close}` を削除する"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })
}

fn delete_pua_action(uri: &Url, diag: &Diagnostic, codepoint: u32) -> CodeActionOrCommand {
    let edits = vec![TextEdit {
        range: diag.range,
        new_text: String::new(),
    }];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("私用領域文字 U+{codepoint:04X} を削除する"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diag.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        is_preferred: Some(true),
        ..CodeAction::default()
    })
}

/// Append a forward-reference `［＃「SEL」に傍点］` immediately after
/// the selection. The selection itself is not modified — bouten
/// targets the prior run.
fn forward_bouten_action(uri: &Url, selection: Range, selected: &str) -> CodeActionOrCommand {
    let new_text = format!("［＃「{selected}」に傍点］");
    let edits = vec![TextEdit {
        range: Range::new(selection.end, selection.end),
        new_text,
    }];
    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    CodeActionOrCommand::CodeAction(CodeAction {
        title: "傍点を付ける ［＃「SEL」に傍点］".to_owned(),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            document_changes: None,
            change_annotations: None,
        }),
        ..CodeAction::default()
    })
}

#[cfg(test)]
mod tests {
    use tower_lsp::lsp_types::Position;

    use super::*;

    fn fake_uri() -> Url {
        Url::parse("file:///fake.afm").expect("valid URL")
    }

    fn extract_change_count(action: &CodeActionOrCommand) -> usize {
        let CodeActionOrCommand::CodeAction(ca) = action else {
            panic!("expected CodeAction");
        };
        ca.edit
            .as_ref()
            .and_then(|e| e.changes.as_ref())
            .and_then(|c| c.values().next())
            .map_or(0, Vec::len)
    }

    #[test]
    fn empty_selection_yields_no_actions() {
        let zero = Range::new(Position::new(0, 0), Position::new(0, 0));
        assert!(wrap_selection_actions("hello", &LineIndex::new("hello"), &fake_uri(), zero).is_empty());
    }

    #[test]
    fn nonempty_selection_returns_full_menu() {
        let src = "青空";
        let sel = Range::new(Position::new(0, 0), Position::new(0, 2));
        let actions = wrap_selection_actions(src, &LineIndex::new(src), &fake_uri(), sel);
        assert_eq!(actions.len(), 7, "expected 7 wrap actions");
    }

    #[test]
    fn wrap_pair_inserts_two_edits() {
        let src = "青空";
        let sel = Range::new(Position::new(0, 0), Position::new(0, 2));
        let actions = wrap_selection_actions(src, &LineIndex::new(src), &fake_uri(), sel);
        for action in actions.iter().take(5) {
            // First five (《 》, 《《 》》, 「 」, 〔 〕, ［＃ ］) wrap
            // with a pair → 2 edits each.
            assert_eq!(extract_change_count(action), 2);
        }
    }

    #[test]
    fn forward_bouten_carries_selected_text() {
        let src = "青空";
        let sel = Range::new(Position::new(0, 0), Position::new(0, 2));
        let actions = wrap_selection_actions(src, &LineIndex::new(src), &fake_uri(), sel);
        let bouten = actions.last().expect("bouten last");
        let CodeActionOrCommand::CodeAction(ca) = bouten else {
            unreachable!()
        };
        let change_text = ca
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()[0]
            .new_text
            .clone();
        assert_eq!(change_text, "［＃「青空」に傍点］");
    }

    #[test]
    fn ruby_base_inserts_pipe_then_empty_reading() {
        let src = "青空";
        let sel = Range::new(Position::new(0, 0), Position::new(0, 2));
        let actions = wrap_selection_actions(src, &LineIndex::new(src), &fake_uri(), sel);
        // Ruby base is the second-to-last action (just before bouten).
        let ruby = &actions[actions.len() - 2];
        let CodeActionOrCommand::CodeAction(ca) = ruby else {
            unreachable!()
        };
        let edits: Vec<&str> = ca
            .edit
            .as_ref()
            .unwrap()
            .changes
            .as_ref()
            .unwrap()
            .values()
            .next()
            .unwrap()
            .iter()
            .map(|e| e.new_text.as_str())
            .collect();
        assert_eq!(edits, vec!["｜", "《》"]);
    }
}
