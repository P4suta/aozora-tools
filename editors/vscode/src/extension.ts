// aozora VS Code extension — launches the aozora-lsp language server over
// stdio and wires it to .afm / .aozora / .aozora.txt documents.

import {
  ExtensionContext,
  window,
  workspace,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  const config = workspace.getConfiguration("aozora");
  const lspPath = config.get<string>("lsp.path", "aozora-lsp");

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
          RUST_LOG: "aozora_lsp=debug",
        },
      },
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "aozora" },
    ],
    synchronize: {
      configurationSection: "aozora",
    },
  };

  client = new LanguageClient(
    "aozora",
    "aozora Language Server",
    serverOptions,
    clientOptions,
  );

  context.subscriptions.push(client);

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
