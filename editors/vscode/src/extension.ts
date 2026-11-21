import { execFile } from "node:child_process";
import { promisify } from "node:util";
import {
  ConfigurationTarget,
  ExtensionContext,
  LogOutputChannel,
  OutputChannel,
  TextDocument,
  Uri,
  ViewColumn,
  WorkspaceConfiguration,
  commands,
  env,
  window,
  workspace,
} from "vscode";
import {
  ClientCapabilities,
  ExecuteCommandParams,
  ExecuteCommandRequest,
  FeatureState,
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  StaticFeature,
} from "vscode-languageclient/node";

const execFileAsync = promisify(execFile);
const RELEASE_URL = "https://github.com/kage1020/Cairn/releases";
const PROBE_TIMEOUT_MS = 5_000;
// Not `cairn.info`: the client registers that one itself, from the server's capabilities.
const SHOW_INFO = "cairn.showInfo";
const INFO = "cairn.info";
const SHOW_DIFF_BLOCKS = "cairn.showDiffBlocks";
const DIFF_BLOCKS = "cairn.diffBlocks";
const STRUCTURE_EXTENSIONS = ["nbt", "litematic", "schem", "mcstructure"];
/**
 * The command a quick-fix runs to pin another target; its arguments are the
 * edition and the version.
 */
const SET_TARGET = "cairn.setTarget";

let client: LanguageClient | undefined;
/** Settles once the server has started, or is known not to start. */
let ready: Promise<void> = Promise.resolve();

/** What a report command did: the report it opened, and the message it showed. */
export interface ReportShown {
  readonly opened?: TextDocument;
  readonly message?: string;
}

export async function activate(context: ExtensionContext): Promise<void> {
  const output = window.createOutputChannel("Cairn Language Server", { log: true });
  context.subscriptions.push(output);
  // Before the server is found, so the commands can say it is missing.
  context.subscriptions.push(commands.registerCommand(SHOW_INFO, showInfo));
  context.subscriptions.push(
    commands.registerCommand(SHOW_DIFF_BLOCKS, showDiffBlocks),
  );
  ready = startServer(context, output);
  await ready;
}

async function startServer(
  context: ExtensionContext,
  output: LogOutputChannel,
): Promise<void> {
  const serverPath = await resolveServerPath(output);
  if (!serverPath) {
    return;
  }

  await logServerVersion(serverPath, output);

  // No `transport`: `TransportKind.stdio` appends `--stdio`, which older
  // `cairn-lsp` releases exit 2 on.
  const serverOptions: ServerOptions = {
    run: { command: serverPath },
    debug: { command: serverPath },
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
  client.registerFeature(new ClientCommands());
  context.subscriptions.push(commands.registerCommand(SET_TARGET, setTarget));

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
    // VS Code is tearing the extension host down; nothing user-facing is left.
    console.error("cairn-lsp: stop() failed during deactivate:", err);
  }
}

/**
 * Opens what `cairn.info` answers for the active `.crn` editor beside it,
 * waiting out a server that is still starting.
 */
async function showInfo(): Promise<ReportShown> {
  const source = await activeSource(
    "Cairn: Show Info",
    "reads the lockfile beside the source",
  );
  if (typeof source === "string") {
    return refuse(source);
  }
  return showReport(source.client, "Cairn: Show Info", "cairn info", {
    command: INFO,
    arguments: [{ uri: source.uri }],
  });
}

/**
 * Opens what `cairn.diffBlocks` answers for the active `.crn` editor and a
 * structure file beside it. The structure file is `structure` when a caller
 * passes one, else the file the user picks.
 */
async function showDiffBlocks(structure?: Uri): Promise<ReportShown> {
  const source = await activeSource(
    "Cairn: Diff Blocks",
    "needs the document's text in cairn-lsp, which the extension sends only for a file",
  );
  if (typeof source === "string") {
    return refuse(source);
  }
  const picked =
    structure ??
    (
      await window.showOpenDialog({
        canSelectMany: false,
        openLabel: "Diff Blocks",
        title: "The structure file to compare the active .crn file with",
        filters: { "Structure files": STRUCTURE_EXTENSIONS },
      })
    )?.[0];
  if (!picked) {
    return {};
  }
  return showReport(source.client, "Cairn: Diff Blocks", "cairn diff-blocks", {
    command: DIFF_BLOCKS,
    arguments: [
      {
        uri: source.uri,
        structure: source.client.code2ProtocolConverter.asUri(picked),
      },
    ],
  });
}

/**
 * The running client and the active `.crn` file's URI for the command `title`
 * names, or why there are none. `whyFile` says why the document must be a
 * saved file.
 */
async function activeSource(
  title: string,
  whyFile: string,
): Promise<{ client: LanguageClient; uri: string } | string> {
  const editor = window.activeTextEditor;
  if (editor?.document.languageId !== "cairn") {
    return `${title} reports on the active editor, which is not a .crn file. Open one first.`;
  }
  if (editor.document.uri.scheme !== "file") {
    return `${title} ${whyFile}. Save the document to a file first.`;
  }
  await ready.catch(() => undefined);
  const current = client;
  if (!current?.isRunning()) {
    return (
      `${title} needs cairn-lsp, which is not running. ` +
      "See the Output panel (Cairn Language Server) for why."
    );
  }
  return {
    client: current,
    uri: current.code2ProtocolConverter.asUri(editor.document.uri),
  };
}

/**
 * Sends `request` to `cairn-lsp` and opens its answer beside the editor as
 * JSON, saying when the answer is the failure document `cli` prints.
 */
async function showReport(
  current: LanguageClient,
  title: string,
  cli: string,
  request: ExecuteCommandParams,
): Promise<ReportShown> {
  let report: unknown;
  try {
    report = await current.sendRequest(ExecuteCommandRequest.type, request);
  } catch (err) {
    return refuse(
      `cairn-lsp answered ${request.command} with an error: ${describeError(err)}`,
    );
  }
  let opened: TextDocument;
  try {
    opened = await workspace.openTextDocument({
      language: "json",
      content: `${JSON.stringify(report, null, 2)}\n`,
    });
    await window.showTextDocument(opened, {
      viewColumn: ViewColumn.Beside,
      preserveFocus: true,
    });
  } catch (err) {
    return refuse(`${title} could not open the report: ${describeError(err)}`);
  }
  const findings = failureFindings(report);
  if (findings === undefined) {
    return { opened };
  }
  const message =
    findings > 0
      ? `${cli} refused the source, so there is no report. ` +
        "The document beside it lists the findings that refused it."
      : `${cli} refused the run as a whole, so there is no report. ` +
        `Run ${cli} in a terminal to see why.`;
  void window.showWarningMessage(message);
  return { opened, message };
}

/** Shows `message` as an error and returns it as what the command did. */
function refuse(message: string): ReportShown {
  void window.showErrorMessage(message);
  return { message };
}

/**
 * How many findings `report` lists when it is the `{"diagnostics": [...]}`
 * document of a refused run, else `undefined`.
 */
function failureFindings(report: unknown): number | undefined {
  if (
    typeof report !== "object" ||
    report === null ||
    Object.keys(report).length !== 1
  ) {
    return undefined;
  }
  const { diagnostics } = report as { diagnostics?: unknown };
  return Array.isArray(diagnostics) ? diagnostics.length : undefined;
}

/**
 * Declares the commands the extension runs for the server, under
 * `experimental.commands`, as no standard client capability lists them.
 */
class ClientCommands implements StaticFeature {
  fillClientCapabilities(capabilities: ClientCapabilities): void {
    const experimental = (capabilities.experimental ?? {}) as Record<
      string,
      unknown
    >;
    capabilities.experimental = {
      ...experimental,
      commands: { commands: [SET_TARGET] },
    };
  }

  initialize(): void {}

  getState(): FeatureState {
    return { kind: "static" };
  }

  clear(): void {}
}

/**
 * Writes `version` to `cairn.target`, which the server reads as the new pin,
 * while `cairn.edition` still names the `edition` the quick-fix was offered in.
 */
async function setTarget(edition: unknown, version: unknown): Promise<void> {
  if (typeof edition !== "string" || typeof version !== "string") {
    void window.showErrorMessage(
      `${SET_TARGET} takes an edition and a version, not ${JSON.stringify([edition, version])}.`,
    );
    return;
  }
  const settings = workspace.getConfiguration("cairn");
  const pinned = settings.get<string>("edition", "").trim();
  if (pinned !== edition) {
    void window.showErrorMessage(
      `The quick-fix pins ${edition} ${version}, but cairn.edition is now ` +
        `${pinned === "" ? "unset" : pinned}. Ask for the quick-fix again.`,
    );
    return;
  }
  try {
    await settings.update("target", version, pinScope(settings));
  } catch (err) {
    void window.showErrorMessage(
      `Could not set cairn.target to ${version}: ${describeError(err)}`,
    );
  }
}

/**
 * The scope that sets `cairn.target`, else the one that sets `cairn.edition`,
 * so the target lands beside the edition it is read with.
 */
function pinScope(settings: WorkspaceConfiguration): ConfigurationTarget {
  const target = settings.inspect("target");
  if (target?.workspaceValue !== undefined) {
    return ConfigurationTarget.Workspace;
  }
  if (target?.globalValue !== undefined) {
    return ConfigurationTarget.Global;
  }
  return settings.inspect("edition")?.workspaceValue !== undefined
    ? ConfigurationTarget.Workspace
    : ConfigurationTarget.Global;
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
    // Not awaited: Show Info waits on the start, not on the reader's answer.
    void promptForInstall();
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
    // `execFile` sets `killed` when the timeout kills the child.
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
