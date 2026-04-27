// aozora VS Code extension — launches the aozora-lsp language server over
// stdio and wires it to .afm / .aozora / .aozora.txt documents, plus
// any plaintext .txt file whose content looks like an aozora-bunko work.

import {
  commands,
  type ExtensionContext,
  languages,
  type TextDocument,
  window,
  workspace,
} from "vscode";
import {
  LanguageClient,
  type LanguageClientOptions,
  type ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

import { registerGaijiFold } from "./gaijiFold";
import { registerNotationGuideCommand } from "./notationGuide";
import { registerPreviewCommand } from "./preview";
import { registerWrapCommands } from "./wrap";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration("aozora");
  const lspPath = resolveVars(config.get<string>("lsp.path", "aozora-lsp"));

  const serverOptions: ServerOptions = {
    run: {
      command: lspPath,
      transport: TransportKind.stdio,
    },
    debug: {
      command: lspPath,
      transport: TransportKind.stdio,
      options: {
        env: {
          ...process.env,
          // biome-ignore lint/style/useNamingConvention: env var name is an external contract
          RUST_LOG: "aozora_lsp=debug",
        },
      },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "aozora" }],
    synchronize: {
      configurationSection: "aozora",
    },
  };

  client = new LanguageClient("aozora", "aozora Language Server", serverOptions, clientOptions);

  context.subscriptions.push(client);

  // Manual opt-in: users can force any open document into the aozora
  // language, handy for .txt scratch buffers that don't trigger the
  // auto-detect heuristic below.
  context.subscriptions.push(
    commands.registerCommand("aozora.setLanguageMode", async () => {
      const editor = window.activeTextEditor;
      if (!editor) {
        void window.showInformationMessage("Open a text editor first, then run this command.");
        return;
      }
      await languages.setTextDocumentLanguage(editor.document, "aozora");
    }),
  );

  // Auto-detect: scan plaintext .txt documents at open time and flip
  // them to "aozora" if the first few kilobytes look like an aozora
  // bunko work. The aozora-bunko input manual doesn't mandate a file
  // extension (工作員 typically save as .txt), so relying on extension
  // alone misses most real-world files.
  const autoDetect = (document: TextDocument) => {
    void maybeSwitchToAozora(document);
  };
  context.subscriptions.push(workspace.onDidOpenTextDocument(autoDetect));
  for (const doc of workspace.textDocuments) {
    autoDetect(doc);
  }

  // Wire the preview pane command BEFORE starting the client — the
  // command itself only sends a request after `client.start()` resolves
  // but registering the disposable up front matches the pattern every
  // other contribution in this file uses.
  registerPreviewCommand(context, client);
  // Wrap-selection commands are pure client-side WorkspaceEdits: no
  // LSP roundtrip needed for trivial open/close splices. The same 7
  // wraps are also exposed via the LSP `code_action` handler so
  // editors that talk LSP only (helix, neovim) get them too.
  registerWrapCommands(context);
  // Inline-fold gaiji spans — `※[#…]` collapses to its resolved
  // glyph when the cursor is elsewhere; expands back when cursor
  // enters the span. Driven by the LSP `aozora/gaijiSpans` request.
  registerGaijiFold(context, client);
  // `Aozora: 記法ガイドを開く` — webview pane rendering the
  // shipped Markdown reference. Discoverable from the command
  // palette, the editor context menu, and the welcome walkthrough.
  registerNotationGuideCommand(context);

  try {
    await client.start();
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    void window.showErrorMessage(
      `aozora-lsp failed to start (${lspPath}): ${message}. ` +
        `Check \`aozora.lsp.path\` in settings, or build aozora-lsp with \`cargo build --release\` and add it to PATH.`,
    );
  }
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

// Minimal VS Code variable resolver for the few substitutions that
// matter inside `aozora.lsp.path` — enough to let settings.json carry
// `${workspaceFolder}/../target/release/aozora-lsp` instead of a hard-
// coded absolute path.
function resolveVars(input: string): string {
  const ws = workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
  return (
    input
      .replace(/\$\{workspaceFolder\}/g, ws)
      // biome-ignore lint/complexity/useLiteralKeys: `process.env` is a Dict<string> index signature; `noPropertyAccessFromIndexSignature` requires bracket access
      .replace(/\$\{userHome\}/g, process.env["HOME"] ?? "")
      .replace(/\$\{env:([A-Za-z_][A-Za-z0-9_]*)\}/g, (_, name) => process.env[name] ?? "")
  );
}

// Aozora-bunko "input manual" feature detection.
//
// We look at the leading ~4 KiB of the buffer for three independent
// signals:
//
//   1. `［＃...］` — any editor annotation.
//   2. `｜X《Y》` — explicit ruby form, cheap to match reliably.
//   3. `X《Y》` — implicit ruby form, matched with a kanji-range base.
//   4. the standard `-------...` header-separator line, ≥ 40 dashes,
//      which the aozora-bunko preamble uses to fence the legend block.
//
// Any one of those is enough; the heuristic is tuned for high precision
// on typical aozora source files rather than maximum recall.
function looksLikeAozora(head: string): boolean {
  if (head.includes("［＃")) {
    return true;
  }
  if (/｜[^《》\n]{1,40}《[^》\n]{1,40}》/.test(head)) {
    return true;
  }
  if (/[\p{Script=Han}々〆ヵヶ]{1,20}《[^》\n]{1,40}》/u.test(head)) {
    return true;
  }
  if (/^-{40,}$/m.test(head)) {
    return true;
  }
  return false;
}

async function maybeSwitchToAozora(document: TextDocument): Promise<void> {
  if (document.languageId !== "plaintext") {
    return;
  }
  if (document.uri.scheme !== "file") {
    return;
  }
  const path = document.uri.fsPath;
  if (!/\.(txt|text)$/i.test(path)) {
    return;
  }
  const enabled = workspace.getConfiguration("aozora").get<boolean>("autoDetect.plaintext", true);
  if (!enabled) {
    return;
  }
  const head = document.getText().slice(0, 4096);
  if (!looksLikeAozora(head)) {
    return;
  }
  await languages.setTextDocumentLanguage(document, "aozora");
}
