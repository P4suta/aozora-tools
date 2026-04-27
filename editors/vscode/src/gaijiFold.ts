// Inline-fold decorations for `※［＃description、mencode］` spans.
//
// VS Code shows the verbose source bytes by default — for prose
// reading that is all noise. The plugin replaces each gaiji span
// with the resolved character so reading flows; the source
// re-appears the moment the cursor enters the span.
//
// ## Architecture
//
// 1. Ask the LSP (`aozora/gaijiSpans` custom request) for every
//    resolvable span. The LSP serves these from a pre-extracted,
//    lock-free cache so the round-trip is microseconds even on
//    100 KB+ documents.
// 2. Bucket the spans by the resolved glyph string. We share one
//    [`TextEditorDecorationType`] per glyph so VS Code's per-type
//    decoration store stays small (`O(distinct glyphs)` rather than
//    `O(spans)`).
// 3. Apply two layers per visible span:
//      - the *hide* layer: the source range is collapsed via CSS
//        `font-size: 0` + `letter-spacing: -1ch` (no API truly
//        removes the chars from the editor model);
//      - the *glyph* layer: an inline `before` decoration shows the
//        resolved character with a subtle dotted underline so the
//        reader sees "this is a fold, click to inspect".
// 4. When the cursor enters a span, that span is omitted from the
//    decoration set so source becomes editable. When the cursor
//    leaves, the next refresh re-folds it.

import {
  type Disposable,
  type ExtensionContext,
  type Position as VsPosition,
  Range,
  type TextEditor,
  type TextEditorDecorationType,
  ThemeColor,
  window,
  workspace,
} from "vscode";
import type { LanguageClient } from "vscode-languageclient/node";

/** Server-side payload for the `aozora/gaijiSpans` LSP request.
 *  Mirrors `aozora_lsp::backend::GaijiSpansResult`. */
interface GaijiSpansResponse {
  readonly spans: ReadonlyArray<GaijiSpanWire>;
}

interface GaijiSpanWire {
  readonly range: { start: VsPositionLike; end: VsPositionLike };
  readonly resolved: string;
  readonly description: string;
  readonly mencode: string | null;
}

interface VsPositionLike {
  readonly line: number;
  readonly character: number;
}

const GAIJI_SPANS_REQUEST = "aozora/gaijiSpans" as const;
const CONFIG_NAMESPACE = "aozora.gaijiFold" as const;

/** Per-extension instance state. Holds the decoration types so we
 *  can reuse them across refreshes (creating a new type per call
 *  would balloon the editor's decoration handle table). */
class FoldState implements Disposable {
  private readonly hideDecoration: TextEditorDecorationType;
  private readonly glyphDecorations = new Map<string, TextEditorDecorationType>();

  constructor() {
    this.hideDecoration = window.createTextEditorDecorationType({
      // No API removes characters from the editor; collapse them
      // visually to zero width via CSS instead.
      textDecoration: "none; font-size: 0px; letter-spacing: -1ch",
    });
  }

  /** Fetch and apply spans for `editor`. Idempotent — safe to call
   *  on every selection / didChange / config event. */
  async refresh(editor: TextEditor, client: LanguageClient): Promise<void> {
    if (editor.document.languageId !== "aozora") return;
    if (!isEnabled()) {
      this.clear(editor);
      return;
    }
    let response: GaijiSpansResponse;
    try {
      response = await client.sendRequest<GaijiSpansResponse>(
        GAIJI_SPANS_REQUEST,
        { uri: editor.document.uri.toString() },
      );
    } catch {
      // Server is briefly unavailable (start-up, mid-restart) — skip
      // this refresh; the next event will retry.
      return;
    }
    const cursor = editor.selection.active;
    const { hideRanges, glyphBuckets } = bucket(response.spans, cursor);
    editor.setDecorations(this.hideDecoration, hideRanges);
    this.applyGlyphBuckets(editor, glyphBuckets);
  }

  /** Clear every decoration this state owns from `editor`. */
  clear(editor: TextEditor): void {
    editor.setDecorations(this.hideDecoration, []);
    for (const deco of this.glyphDecorations.values()) {
      editor.setDecorations(deco, []);
    }
  }

  dispose(): void {
    this.hideDecoration.dispose();
    for (const deco of this.glyphDecorations.values()) deco.dispose();
    this.glyphDecorations.clear();
  }

  private applyGlyphBuckets(
    editor: TextEditor,
    buckets: ReadonlyMap<string, ReadonlyArray<Range>>,
  ): void {
    const seen = new Set<string>();
    for (const [glyph, ranges] of buckets) {
      seen.add(glyph);
      const deco =
        this.glyphDecorations.get(glyph) ??
        this.glyphDecorations
          .set(glyph, createGlyphDecoration(glyph))
          .get(glyph)!;
      editor.setDecorations(deco, ranges);
    }
    // Glyphs that disappeared this cycle: clear their ranges so the
    // editor stops drawing them. We keep the decoration type itself
    // around because the same glyph is likely to reappear soon.
    for (const [glyph, deco] of this.glyphDecorations) {
      if (!seen.has(glyph)) editor.setDecorations(deco, []);
    }
  }
}

function bucket(
  spans: ReadonlyArray<GaijiSpanWire>,
  cursor: VsPosition,
): {
  hideRanges: Range[];
  glyphBuckets: Map<string, Range[]>;
} {
  const hideRanges: Range[] = [];
  const glyphBuckets = new Map<string, Range[]>();
  for (const span of spans) {
    const range = new Range(
      span.range.start.line,
      span.range.start.character,
      span.range.end.line,
      span.range.end.character,
    );
    // Spans containing the cursor stay un-folded so the user can
    // edit. The next selection-change event will re-fold once the
    // cursor moves out.
    if (range.contains(cursor)) continue;
    hideRanges.push(range);
    const existing = glyphBuckets.get(span.resolved);
    if (existing) {
      existing.push(range);
    } else {
      glyphBuckets.set(span.resolved, [range]);
    }
  }
  return { hideRanges, glyphBuckets };
}

function createGlyphDecoration(resolved: string): TextEditorDecorationType {
  return window.createTextEditorDecorationType({
    before: {
      contentText: resolved,
      color: new ThemeColor("editor.foreground"),
      textDecoration: "underline dotted rgba(128,128,128,0.6)",
      margin: "0",
    },
  });
}

function isEnabled(): boolean {
  return workspace.getConfiguration(CONFIG_NAMESPACE).get<boolean>("enabled", true);
}

export function registerGaijiFold(
  context: ExtensionContext,
  client: LanguageClient,
): void {
  const state = new FoldState();
  context.subscriptions.push(state);

  const refresh = (editor: TextEditor | undefined) => {
    if (!editor) return;
    void state.refresh(editor, client);
  };

  context.subscriptions.push(
    window.onDidChangeActiveTextEditor((editor) => refresh(editor)),
    workspace.onDidChangeTextDocument((event) => {
      const editor = window.activeTextEditor;
      if (editor && editor.document === event.document) refresh(editor);
    }),
    window.onDidChangeTextEditorSelection((event) => refresh(event.textEditor)),
    workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration(CONFIG_NAMESPACE)) {
        refresh(window.activeTextEditor);
      }
    }),
  );

  // Initial pass over editors that were already open when the
  // extension activated.
  for (const editor of window.visibleTextEditors) refresh(editor);
}
