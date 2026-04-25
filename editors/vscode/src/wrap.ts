// Selection-wrap commands — surrounds the active editor's selection
// with one of the seven aozora-notation delimiter pairs.
//
// These mirror the LSP `code_action` wraps in
// `crates/aozora-lsp/src/code_actions.rs`, but run client-side so:
//   * they show up directly in the right-click context menu (not
//     buried under "Refactor"), matching the JetBrains
//     "Surround With" pattern;
//   * they have keybindings (Ctrl+Alt+R for ruby, Ctrl+Alt+B for
//     bouten);
//   * there's no LSP roundtrip latency for a trivial splice.
//
// The LSP path stays as the canonical, editor-agnostic API. This
// file is the VS Code-specific UX promotion.

import {
  commands,
  ExtensionContext,
  Position,
  Range,
  Selection,
  TextEditor,
  TextEditorEdit,
  window,
} from "vscode";

interface WrapShape {
  readonly id: string;
  readonly apply: (editor: TextEditor, edit: TextEditorEdit, sel: Selection) => Selection | null;
}

const WRAPS: ReadonlyArray<WrapShape> = [
  pairWrap("aozora.wrap.ruby", "《", "》"),
  pairWrap("aozora.wrap.doubleRuby", "《《", "》》"),
  pairWrap("aozora.wrap.kagikakko", "「", "」"),
  pairWrap("aozora.wrap.kikkou", "〔", "〕"),
  pairWrap("aozora.wrap.chuki", "［＃", "］"),
  // Ruby base: insert ｜ before the selection, 《》 after, and place
  // the cursor inside the empty reading slot so the user can type.
  {
    id: "aozora.wrap.rubyBase",
    apply: (_editor, edit, sel) => {
      edit.insert(sel.start, "｜");
      edit.insert(sel.end, "《》");
      // After both edits, the empty 《》 sits at sel.end + 1 char
      // shift. We can't compute the post-edit position synchronously
      // here, so we leave cursor placement to a follow-up
      // selection-set on the editor (caller does it).
      return null;
    },
  },
  // Forward-reference bouten — append `［＃「SEL」に傍点］` after the
  // selection, leaving the selection itself untouched.
  {
    id: "aozora.wrap.bouten",
    apply: (editor, edit, sel) => {
      const selected = editor.document.getText(sel);
      edit.insert(sel.end, `［＃「${selected}」に傍点］`);
      return null;
    },
  },
];

export function registerWrapCommands(context: ExtensionContext): void {
  for (const wrap of WRAPS) {
    context.subscriptions.push(
      commands.registerCommand(wrap.id, async () => {
        const editor = window.activeTextEditor;
        if (!editor || editor.selections.every((s) => s.isEmpty)) {
          void window.showInformationMessage(
            "選択範囲がありません。マウスやキーボードで囲みたい範囲を選んでから実行してください。",
          );
          return;
        }
        await editor.edit((edit) => {
          for (const sel of editor.selections) {
            if (sel.isEmpty) {
              continue;
            }
            wrap.apply(editor, edit, sel);
          }
        });
      }),
    );
  }
}

// Build a "wrap selection in open/close pair" command shape.
function pairWrap(id: string, open: string, close: string): WrapShape {
  return {
    id,
    apply: (_editor, edit, sel) => {
      // Insert close FIRST so the start-position offset for the open
      // insert remains correct relative to the original buffer
      // (VS Code's `TextEditorEdit` accepts edits in any order, but
      // this ordering matches the underlying `WorkspaceEdit` builder
      // most cleanly).
      edit.insert(sel.end, close);
      edit.insert(sel.start, open);
      return new Selection(
        new Position(sel.start.line, sel.start.character + open.length),
        new Position(sel.end.line, sel.end.character + open.length),
      );
    },
  };
}
