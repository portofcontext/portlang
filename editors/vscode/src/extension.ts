import * as path from 'path';
import * as fs from 'fs';
import * as vscode from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

let client: LanguageClient;

function findServerExecutable(context: vscode.ExtensionContext): string | undefined {
  // 1. Bundled binary (published extension)
  const bundled = path.join(context.extensionPath, 'bin', 'portlang-lsp');
  if (fs.existsSync(bundled)) {
    try { fs.chmodSync(bundled, 0o755); } catch (_) {}
    return bundled;
  }

  // 2. Development: cargo build output relative to workspace folders
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    const dev = path.join(folder.uri.fsPath, 'target', 'debug', 'portlang-lsp');
    if (fs.existsSync(dev)) {
      return dev;
    }
  }

  // 3. PATH (brew install portlang)
  // The client will try to find it in PATH by name
  return 'portlang-lsp';
}

export function activate(context: vscode.ExtensionContext): void {
  const serverExecutable = findServerExecutable(context);

  const serverOptions: ServerOptions = {
    run: {
      command: serverExecutable!,
      transport: TransportKind.stdio,
    },
    debug: {
      command: serverExecutable!,
      transport: TransportKind.stdio,
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: 'file', language: 'field' }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.field'),
    },
  };

  client = new LanguageClient(
    'portlang',
    'portlang Field Language Server',
    serverOptions,
    clientOptions
  );

  client.start();
  context.subscriptions.push(client);
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
