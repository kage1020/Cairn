// Cairn VS Code extension — spawns `cairn-lsp` and wires it up as an LSP
// client. Keeps the surface minimal: resolve a server binary, launch it over
// stdio, forward diagnostics and completions the server already emits.

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  ExtensionContext,
  OutputChannel,
  Uri,
  env,
  window,
  workspace,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

const execFileAsync = promisify(execFile);
const RELEASE_URL = "https://github.com/kage1020/Cairn/releases";

let client: LanguageClient | undefined;

export async function activate(context: ExtensionContext): Promise<void> {
  const output = window.createOutputChannel("Cairn Language Server");
  context.subscriptions.push(output);

  const serverPath = await resolveServerPath(output);
  if (!serverPath) {
    return;
  }

  await logServerVersion(serverPath, output);

  const serverOptions: ServerOptions = {
    run: { command: serverPath, transport: TransportKind.stdio },
    debug: { command: serverPath, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "cairn" }],
    outputChannel: output,
    synchronize: {
      configurationSection: "cairn",
    },
  };

  client = new LanguageClient(
    "cairn",
    "Cairn Language Server",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    void window.showErrorMessage(
      `Failed to start cairn-lsp (${serverPath}): ${detail}. ` +
        "Check the Output panel (Cairn Language Server) for details.",
    );
    output.appendLine(`start failure: ${detail}`);
  }
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
    client = undefined;
  }
}

// Resolution order: explicit setting → PATH lookup → guided error. Never
// silent — a missing server is the single most common install failure and
// silent no-op editing looks identical to "extension broken".
async function resolveServerPath(
  output: OutputChannel,
): Promise<string | undefined> {
  const configured = workspace
    .getConfiguration("cairn")
    .get<string>("serverPath", "")
    .trim();

  if (configured.length > 0) {
    if (await isExecutable(configured)) {
      output.appendLine(`using cairn.serverPath: ${configured}`);
      return configured;
    }
    void window.showErrorMessage(
      `cairn.serverPath is set to \`${configured}\` but the file is not ` +
        "executable. Fix the setting or clear it to fall back to PATH lookup.",
    );
    return undefined;
  }

  // Rely on the OS PATH search built into child_process spawn: `cairn-lsp`
  // resolves the same way `cairn` and any other CLI does. `execFile` returns
  // ENOENT with a clear code when the binary is missing.
  const command = process.platform === "win32" ? "cairn-lsp.exe" : "cairn-lsp";
  if (await isOnPath(command)) {
    output.appendLine(`using cairn-lsp from PATH (${command})`);
    return command;
  }

  const choice = await window.showErrorMessage(
    "cairn-lsp was not found on PATH and cairn.serverPath is unset. " +
      "Install the binary from the Cairn GitHub release matching your cairn CLI.",
    "Open release page",
  );
  if (choice === "Open release page") {
    void env.openExternal(Uri.parse(RELEASE_URL));
  }
  return undefined;
}

async function isExecutable(path: string): Promise<boolean> {
  try {
    await execFileAsync(path, ["--version"], { timeout: 5_000 });
    return true;
  } catch {
    return false;
  }
}

async function isOnPath(command: string): Promise<boolean> {
  try {
    await execFileAsync(command, ["--version"], { timeout: 5_000 });
    return true;
  } catch {
    return false;
  }
}

// Log the server version at activation so bug reports can be correlated
// with a specific Cairn release without asking the user to re-run commands.
async function logServerVersion(
  serverPath: string,
  output: OutputChannel,
): Promise<void> {
  try {
    const { stdout } = await execFileAsync(serverPath, ["--version"], {
      timeout: 5_000,
    });
    output.appendLine(`server: ${stdout.trim()}`);
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    output.appendLine(`server: --version probe failed (${detail})`);
  }
}
