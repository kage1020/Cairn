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
const PROBE_TIMEOUT_MS = 5_000;

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
    const detail = describeError(err);
    output.appendLine(`start failure: ${detail}`);
    void window.showErrorMessage(
      `Failed to start cairn-lsp (${serverPath}): ${detail}. ` +
        "See the Output panel (Cairn Language Server) for details.",
    );
  }
}

export async function deactivate(): Promise<void> {
  const current = client;
  client = undefined;
  if (!current) {
    return;
  }
  try {
    await current.stop();
  } catch (err) {
    // Nothing user-facing left; VS Code is tearing the extension host down.
    // Route the error to the developer console rather than swallow it silent.
    console.error("cairn-lsp: stop() failed during deactivate:", err);
  }
}

async function resolveServerPath(
  output: OutputChannel,
): Promise<string | undefined> {
  const configured = workspace
    .getConfiguration("cairn")
    .get<string>("serverPath", "")
    .trim();

  if (configured.length > 0) {
    const probe = await probeExecutable(configured);
    if (probe.ok) {
      output.appendLine(`using cairn.serverPath: ${configured}`);
      return configured;
    }
    output.appendLine(
      `cairn.serverPath (${configured}) rejected: ${probe.detail}`,
    );
    void window.showErrorMessage(
      probeFailureMessage(
        `cairn.serverPath is set to \`${configured}\` but the binary could not be launched`,
        probe,
      ),
    );
    return undefined;
  }

  const command = process.platform === "win32" ? "cairn-lsp.exe" : "cairn-lsp";
  const probe = await probeExecutable(command);
  if (probe.ok) {
    output.appendLine(`using cairn-lsp from PATH (${command})`);
    return command;
  }
  output.appendLine(
    `cairn-lsp PATH lookup failed (${command}): ${probe.detail}`,
  );

  if (probe.category === "not-found") {
    await promptForInstall();
    return undefined;
  }

  void window.showErrorMessage(
    probeFailureMessage(
      `cairn-lsp is on PATH but could not be launched (${command})`,
      probe,
    ),
  );
  return undefined;
}

async function promptForInstall(): Promise<void> {
  const choice = await window.showErrorMessage(
    "cairn-lsp was not found on PATH and cairn.serverPath is unset. " +
      "Install the binary from the Cairn GitHub release matching your cairn CLI.",
    "Open release page",
  );
  if (choice !== "Open release page") {
    return;
  }
  const opened = await env.openExternal(Uri.parse(RELEASE_URL));
  if (!opened) {
    void window.showWarningMessage(
      `Could not open a browser. Visit ${RELEASE_URL} manually.`,
    );
  }
}

type ProbeCategory = "not-found" | "permission" | "timeout" | "runtime";

type ProbeResult =
  | { readonly ok: true }
  | {
      readonly ok: false;
      readonly category: ProbeCategory;
      readonly detail: string;
    };

async function probeExecutable(command: string): Promise<ProbeResult> {
  try {
    await execFileAsync(command, ["--version"], { timeout: PROBE_TIMEOUT_MS });
    return { ok: true };
  } catch (err) {
    return classifyProbeFailure(err);
  }
}

function classifyProbeFailure(err: unknown): ProbeResult {
  const detail = describeError(err);
  const record = err as NodeJS.ErrnoException & { killed?: boolean };
  if (record?.code === "ENOENT") {
    return { ok: false, category: "not-found", detail };
  }
  if (record?.code === "EACCES" || record?.code === "EPERM") {
    return { ok: false, category: "permission", detail };
  }
  if (record?.killed === true) {
    // `execFile` sets `killed = true` when it kills the child on the timeout.
    return { ok: false, category: "timeout", detail };
  }
  return { ok: false, category: "runtime", detail };
}

function probeFailureMessage(
  prefix: string,
  probe: Exclude<ProbeResult, { ok: true }>,
): string {
  const advice = ((): string => {
    switch (probe.category) {
      case "not-found":
        return "Install the binary or clear the setting to fall back to PATH lookup.";
      case "permission":
        return "Check the file's executable bit and your file-system permissions.";
      case "timeout":
        return "The binary did not respond within 5 seconds — it may be hung or the wrong file.";
      case "runtime":
        return "Run the command manually to see the underlying error.";
    }
  })();
  return `${prefix}: ${probe.detail}. ${advice}`;
}

function describeError(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}

async function logServerVersion(
  serverPath: string,
  output: OutputChannel,
): Promise<void> {
  try {
    const { stdout } = await execFileAsync(serverPath, ["--version"], {
      timeout: PROBE_TIMEOUT_MS,
    });
    output.appendLine(`server: ${stdout.trim()}`);
  } catch (err) {
    const detail = describeError(err);
    output.appendLine(`server: --version probe failed (${detail})`);
    void window.showWarningMessage(
      `cairn-lsp is running but --version failed (${detail}). ` +
        "The server may be from a different Cairn release than expected.",
    );
  }
}
