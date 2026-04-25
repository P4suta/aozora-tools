// Inline-fold decorations for `※［＃description、mencode］` spans.
//
// The VS Code editor by default shows the verbose source bytes
// — `※［＃「木＋吶のつくり」、第3水準1-85-54］` for a single 枘.
// For prose reading that is *all* noise; what the typesetter
// wants to see is the resolved character.
//
// Implementation: ask the LSP for every resolvable gaiji span via
// the custom `aozora/gaijiSpans` request, then decorate each span
// with two layers:
//
//   - source range: hidden via CSS `font-size: 0` + `letter-spacing: -1ch`
//     (no API to truly remove the chars from the editor model;
//     the closest visual approximation is to collapse them to zero
//     width)
//   - inline `before` text: shows the resolved glyph in the same
//     position, with a subtle underline to signal "this is a fold,
//     click to inspect"
//
// When the cursor enters a span, the decoration is removed for
// that range so the source is fully visible & editable. When the
// cursor leaves, the decoration is reinstated.
//
// The set of spans is refreshed on every editor change (via the
// LSP `aozora/gaijiSpans` request, which reads from the
// pre-extracted span cache and runs in microseconds).

import {
  ExtensionContext,
  Range,
  TextEditor,
  TextEditorDecorationType,
  ThemeColor,
  window,
  workspace,
} from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

interface GaijiSpan {
  range: { start: { line: number; character: number }; end: { line: number; character: number } };
  resolved: string;
  description: string;
  mencode: string | null;
}

interface GaijiSpansResult {
  spans: GaijiSpan[];
}

const HIDDEN_DECORATION_RENDER_OPTS = {
  // Collapse the source visually. There is no API to truly
  // hide characters in VS Code; the standard trick is to set
  // font-size 0 + letter-spacing -1ch so the glyphs render at
  // zero width.
  textDecoration: "none; font-size: 0px; letter-spacing: -1ch",
};

let foldedDecoration: TextEditorDecorationType | undefined;

function createFoldedDecoration(): TextEditorDecorationType {
  return window.createTextEditorDecorationType({
    ...HIDDEN_DECORATION_RENDER_OPTS,
    rangeBehavior: 1, // ClosedClosed — decoration shrinks if user edits inside
  });
}

function createGlyphDecoration(resolved: string): TextEditorDecorationType {
  return window.createTextEditorDecorationType({
    before: {
      contentText: resolved,
      // Use a subtle hint color so it's recognisably "synthetic".
      // Falls back to the default editor foreground.
      color: new ThemeColor("editor.foreground"),
      textDecoration: "underline dotted rgba(128,128,128,0.6)",
      margin: "0",
    },
  });
}

export function registerGaijiFold(
  context: ExtensionContext,
  client: LanguageClient,
): void {
  // One decoration *type* per resolved glyph string — VS Code keys
  // decorations by type, not by range, so we share types across
  // multiple ranges that resolve to the same glyph (e.g., 50
  // copies of 枘 share one decoration type).
  const glyphDecorations = new Map<string, TextEditorDecorationType>();

  // Start the per-doc state with cursor-aware decoration removal
  // wired up.
  const refresh = async (editor: TextEditor) => {
    if (editor.document.languageId !== "aozora") return;
    if (!enabled()) {
      clearAll(editor, glyphDecorations);
      return;
    }
    let result: GaijiSpansResult;
    try {
      result = await client.sendRequest("aozora/gaijiSpans", {
        uri: editor.document.uri.toString(),
      });
    } catch {
      return;
    }

    // Bucket the spans by resolved glyph so we apply one
    // `setDecorations` call per decoration type.
    const cursorPos = editor.selection.active;
    const hiddenRanges: Range[] = [];
    const glyphBuckets = new Map<string, Range[]>();
    for (const span of result.spans) {
      const range = new Range(
        span.range.start.line,
        span.range.start.character,
        span.range.end.line,
        span.range.end.character,
      );
      // Skip the fold while the cursor sits inside the span — the
      // user wants to see / edit the source.
      if (range.contains(cursorPos)) continue;
      hiddenRanges.push(range);
      let bucket = glyphBuckets.get(span.resolved);
      if (!bucket) {
        bucket = [];
        glyphBuckets.set(span.resolved, bucket);
      }
      bucket.push(range);
    }

    if (!foldedDecoration) {
      foldedDecoration = createFoldedDecoration();
      context.subscriptions.push(foldedDecoration);
    }
    editor.setDecorations(foldedDecoration, hiddenRanges);

    // Apply per-glyph decorations. For glyphs that appeared in a
    // previous refresh but not this one, set their bucket to
    // empty so VS Code clears them.
    const seen = new Set<string>();
    for (const [glyph, ranges] of glyphBuckets) {
      seen.add(glyph);
      let deco = glyphDecorations.get(glyph);
      if (!deco) {
        deco = createGlyphDecoration(glyph);
        context.subscriptions.push(deco);
        glyphDecorations.set(glyph, deco);
      }
      editor.setDecorations(deco, ranges);
    }
    for (const [glyph, deco] of glyphDecorations) {
      if (!seen.has(glyph)) editor.setDecorations(deco, []);
    }
  };

  // Refresh on:
  //  - active editor change (open file)
  //  - text edit (gaiji table changed shape)
  //  - selection change (cursor entered/left a span)
  context.subscriptions.push(
    window.onDidChangeActiveTextEditor((editor) => {
      if (editor) void refresh(editor);
    }),
    workspace.onDidChangeTextDocument((e) => {
      const editor = window.activeTextEditor;
      if (editor && editor.document === e.document) void refresh(editor);
    }),
    window.onDidChangeTextEditorSelection((e) => {
      void refresh(e.textEditor);
    }),
    workspace.onDidChangeConfiguration((e) => {
      if (!e.affectsConfiguration("aozora.gaijiFold")) return;
      const editor = window.activeTextEditor;
      if (editor) void refresh(editor);
    }),
  );

  // Initial pass over already-open editors.
  for (const editor of window.visibleTextEditors) {
    void refresh(editor);
  }
}

function enabled(): boolean {
  return workspace.getConfiguration("aozora").get<boolean>("gaijiFold.enabled", true);
}

function clearAll(
  editor: TextEditor,
  glyphDecorations: Map<string, TextEditorDecorationType>,
): void {
  if (foldedDecoration) editor.setDecorations(foldedDecoration, []);
  for (const deco of glyphDecorations.values()) {
    editor.setDecorations(deco, []);
  }
}
