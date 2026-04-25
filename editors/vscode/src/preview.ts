// aozora preview pane — Phase 3.1 of the editor-integration sprint.
//
// `aozora.openPreview` opens (or focuses) a side-by-side WebviewPanel
// that shows the rendered HTML output for the active aozora document.
// Re-renders on `onDidChangeTextDocument` (debounced ~150 ms) by
// asking the LSP server through the custom `aozora/renderHtml`
// request.

import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

interface RenderHtmlResult {
  html: string;
}

/**
 * Hidden state per WebviewPanel — tracks the document URI it follows
 * and a debounce handle so rapid edits coalesce into a single render.
 */
interface PreviewState {
  panel: vscode.WebviewPanel;
  uri: vscode.Uri;
  debounce?: NodeJS.Timeout;
}

const PREVIEW_RENDER_DEBOUNCE_MS = 150;

const panelsByUri: Map<string, PreviewState> = new Map();

export function registerPreviewCommand(
  context: vscode.ExtensionContext,
  client: LanguageClient,
): void {
  context.subscriptions.push(
    vscode.commands.registerCommand("aozora.openPreview", async () => {
      const editor = vscode.window.activeTextEditor;
      if (!editor) {
        void vscode.window.showInformationMessage(
          "Open an aozora document first, then run this command.",
        );
        return;
      }
      await openPreview(editor.document, client, context);
    }),
  );

  context.subscriptions.push(
    vscode.workspace.onDidChangeTextDocument((event) => {
      const state = panelsByUri.get(event.document.uri.toString());
      if (!state) {
        return;
      }
      if (state.debounce) {
        clearTimeout(state.debounce);
      }
      state.debounce = setTimeout(() => {
        void renderInto(state, client);
      }, PREVIEW_RENDER_DEBOUNCE_MS);
    }),
  );
}

async function openPreview(
  document: vscode.TextDocument,
  client: LanguageClient,
  context: vscode.ExtensionContext,
): Promise<void> {
  const key = document.uri.toString();
  const existing = panelsByUri.get(key);
  if (existing) {
    existing.panel.reveal(vscode.ViewColumn.Beside);
    await renderInto(existing, client);
    return;
  }

  const panel = vscode.window.createWebviewPanel(
    "aozoraPreview",
    `Aozora Preview — ${document.fileName.split("/").pop() ?? "untitled"}`,
    vscode.ViewColumn.Beside,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
    },
  );

  const state: PreviewState = { panel, uri: document.uri };
  panelsByUri.set(key, state);

  panel.onDidDispose(
    () => {
      const live = panelsByUri.get(key);
      if (live && live.debounce) {
        clearTimeout(live.debounce);
      }
      panelsByUri.delete(key);
    },
    undefined,
    context.subscriptions,
  );

  await renderInto(state, client);
}

async function renderInto(
  state: PreviewState,
  client: LanguageClient,
): Promise<void> {
  try {
    const result = await client.sendRequest<RenderHtmlResult>(
      "aozora/renderHtml",
      { uri: state.uri.toString() },
    );
    state.panel.webview.html = wrapHtml(result.html ?? "");
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    state.panel.webview.html = wrapHtml(
      `<pre>aozora/renderHtml failed: ${escapeHtml(message)}</pre>`,
    );
  }
}

/**
 * Wrap the LSP-rendered body fragment in a minimal HTML5 host page
 * with a stylesheet that gives 青空文庫 prose a comfortable default
 * appearance — generous line-height, light background, vertical
 * scrolling preserved. Vertical-writing toggle is a follow-on.
 */
function wrapHtml(body: string): string {
  return `<!DOCTYPE html>
<html lang="ja">
<head>
  <meta charset="utf-8" />
  <title>Aozora Preview</title>
  <style>
    body {
      font-family: "Hiragino Mincho ProN", "Yu Mincho", serif;
      line-height: 1.9;
      max-width: 42em;
      margin: 1.5em auto;
      padding: 0 1em;
      color: #222;
      background: #fdf6e3;
    }
    rt {
      font-size: 0.55em;
      letter-spacing: 0.05em;
    }
    .aozora_gaiji {
      background: #fff7d6;
      padding: 0 0.1em;
      border-radius: 0.15em;
    }
    h1, h2, h3 {
      font-weight: 600;
      letter-spacing: 0.05em;
      margin-top: 2em;
    }
    p { margin: 0.5em 0; }
  </style>
</head>
<body>
${body}
</body>
</html>`;
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
