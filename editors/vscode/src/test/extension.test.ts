import assert from "node:assert/strict";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, delimiter, dirname, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { test, type TestContext } from "node:test";
import { setTimeout } from "node:timers/promises";
import { runTests } from "@vscode/test-electron";
import type { Pin, ServerEvent } from "./server-event";

const EXTENSION_ROOT = resolve(__dirname, "..", "..");
const FAKE_SERVER = join(__dirname, "fake-cairn-lsp.js");
// CI's `.vscode-test` cache key is read from this line.
const VSCODE_VERSION = "1.143.1";
const EXIT_TIMEOUT_MS = 10_000;
const SERVER_LOG = "Cairn Language Server.log";
const LIFECYCLE = ["initialize", "initialized", "shutdown", "exit"];
const SET_TARGET = "cairn.setTarget";
/** The pin the settings carry while neither `cairn.edition` nor `cairn.target` is set. */
const UNPINNED: Pin = { edition: "", target: "" };
const POSIX_ONLY = {
  skip:
    process.platform === "win32" &&
    "the stand-in cairn-lsp is a POSIX shell script",
};

/**
 * One run of a stand-in `cairn-lsp`: the path the spawn resolved, its
 * arguments, the lifecycle messages it received, the client commands its
 * `initialize` declared, the pins the configuration changes carried, and its
 * exit code.
 */
interface Launch {
  readonly server: string;
  readonly argv: string[];
  readonly received: string[];
  commands: string[];
  readonly pins: Pin[];
  exitCode?: number;
}

/** A `--version` probe of `server`, which receives no message. */
function probe(server: string): Launch {
  return {
    server,
    argv: ["--version"],
    received: [],
    commands: [],
    pins: [],
    exitCode: 0,
  };
}

/** A session with `server`, which receives the lifecycle and `pins`. */
function session(server: string, pins: Pin[]): Launch {
  return {
    server,
    argv: [],
    received: LIFECYCLE,
    commands: [SET_TARGET],
    pins,
    exitCode: 0,
  };
}

/** What one run of VS Code is given. */
interface Run {
  /** The user settings. */
  readonly user: Record<string, unknown>;
  /**
   * The settings of a folder VS Code opens around the document; without them
   * it opens the document alone.
   */
  readonly workspace?: Record<string, unknown>;
  /** The edition and version `cairn.setTarget` runs with once the extension is active. */
  readonly setTarget?: readonly [string, string];
  /** The file `Cairn: Show Info` writes what it opens to, once the extension is active. */
  readonly showInfo?: string;
  /**
   * The file `Cairn: Diff Blocks` writes what it opens to, and the structure
   * file it compares the document with, once the extension is active.
   */
  readonly diffBlocks?: { readonly shown: string; readonly structure: string };
}

/**
 * A temporary directory holding a folder with a `.crn` document, the record
 * the stand-ins append to, and a stand-in `cairn-lsp` for `PATH`.
 */
class Fixture {
  readonly root = mkdtempSync(join(tmpdir(), "cairn-vscode-"));
  readonly folder = join(this.root, "workspace");
  readonly document = join(this.folder, "main.crn");
  readonly record = join(this.root, "launches.jsonl");
  readonly userData = join(this.root, "user-data");
  readonly userSettings = join(this.userData, "User", "settings.json");
  readonly workspaceSettings = join(this.folder, ".vscode", "settings.json");
  readonly pathServer: string;
  private readonly t: TestContext;

  constructor(t: TestContext) {
    this.t = t;
    t.after(() => rmSync(this.root, { recursive: true, force: true }));
    mkdirSync(this.folder);
    writeFileSync(this.document, "");
    writeFileSync(this.record, "");
    this.pathServer = this.server("path");
  }

  /** Writes a stand-in `cairn-lsp` under `dir` and returns its path. */
  server(dir: string): string {
    const path = join(this.root, dir, "cairn-lsp");
    mkdirSync(dirname(path));
    const command = [process.execPath, FAKE_SERVER, this.record]
      .map(shellQuote)
      .join(" ");
    // `$0` is the path the spawn resolved, naming the stand-in that ran.
    const script = `#!/bin/sh\nexec ${command} "$0" "$@"\n`;
    writeFileSync(path, script, { mode: 0o755 });
    return path;
  }

  /**
   * Runs VS Code as `run` says and asserts the stand-ins' launches equal
   * `expected`, reporting the record and the server log if not.
   */
  async expectLaunches(run: Run, expected: Launch[]): Promise<void> {
    const failure = await this.open(run).then(
      () => undefined,
      (error: unknown) => error,
    );
    try {
      assert.deepEqual(await this.launches(), expected);
      if (failure) {
        throw failure;
      }
    } catch (error) {
      if (failure && failure !== error) {
        this.t.diagnostic(`VS Code also failed: ${String(failure)}`);
      }
      this.report();
      throw error;
    }
  }

  /** Runs VS Code on the document with the extension, as `run` says, until it exits. */
  private async open(run: Run): Promise<void> {
    mkdirSync(dirname(this.userSettings), { recursive: true });
    writeFileSync(this.userSettings, JSON.stringify(run.user));
    const opened = [this.document];
    if (run.workspace) {
      mkdirSync(dirname(this.workspaceSettings));
      writeFileSync(this.workspaceSettings, JSON.stringify(run.workspace));
      opened.unshift(this.folder);
    }
    await runTests({
      version: VSCODE_VERSION,
      extensionDevelopmentPath: EXTENSION_ROOT,
      extensionTestsPath: join(__dirname, "host.js"),
      launchArgs: [
        ...opened,
        "--disable-extensions",
        // A folder would otherwise open in Restricted Mode, which disables
        // the extension.
        "--disable-workspace-trust",
        // Otherwise VS Code takes `PATH` from a login shell, whose profile
        // could put an installed `cairn-lsp` ahead of the stand-in.
        "--force-disable-user-env",
        `--user-data-dir=${this.userData}`,
        `--extensions-dir=${join(this.root, "extensions")}`,
      ],
      extensionTestsEnv: {
        PATH: [dirname(this.pathServer), process.env.PATH].join(delimiter),
        CAIRN_TEST_RECORD: this.record,
        CAIRN_TEST_SET_TARGET: run.setTarget && JSON.stringify(run.setTarget),
        CAIRN_TEST_SHOW_INFO: run.showInfo,
        CAIRN_TEST_DIFF_BLOCKS: run.diffBlocks?.shown,
        CAIRN_TEST_STRUCTURE: run.diffBlocks?.structure,
      },
    });
  }

  /**
   * The stand-ins' launches in the order they started, once each has exited.
   *
   * @throws if a stand-in records no exit within `EXIT_TIMEOUT_MS`.
   */
  private async launches(): Promise<Launch[]> {
    const deadline = Date.now() + EXIT_TIMEOUT_MS;
    for (;;) {
      const text = readFileSync(this.record, "utf8");
      const launches = new Map<string, Launch>();
      const running = new Set<string>();
      for (const line of text.split("\n")) {
        if (line.length === 0) {
          continue;
        }
        const event = JSON.parse(line) as ServerEvent;
        if ("argv" in event) {
          const { server, argv } = event;
          const launch = { server, argv, received: [], commands: [], pins: [] };
          launches.set(event.id, launch);
          running.add(event.id);
        } else if ("received" in event) {
          launches.get(event.id)?.received.push(event.received);
        } else if ("commands" in event) {
          const launch = launches.get(event.id);
          if (launch) {
            launch.commands = event.commands;
          }
        } else if ("pin" in event) {
          launches.get(event.id)?.pins.push(event.pin);
        } else if ("exited" in event) {
          const launch = launches.get(event.id);
          if (launch) {
            launch.exitCode = event.exited;
          }
          running.delete(event.id);
        } else {
          event satisfies never;
          throw new Error(`unknown line in ${this.record}: ${line}`);
        }
      }
      if (running.size === 0) {
        return [...launches.values()];
      }
      if (Date.now() > deadline) {
        const ids = [...running].join(", ");
        throw new Error(
          `stand-ins ${ids} recorded no exit within ${EXIT_TIMEOUT_MS} ms of VS Code exiting; record:\n${text}`,
        );
      }
      await setTimeout(100);
    }
  }

  /** Reports the record and every server log VS Code wrote as diagnostics. */
  private report(): void {
    this.attach(this.record);
    const logs = join(this.userData, "logs");
    const entries = existsSync(logs)
      ? readdirSync(logs, { recursive: true, encoding: "utf8" })
      : [];
    const serverLogs = entries.filter(
      (entry) => basename(entry) === SERVER_LOG,
    );
    if (serverLogs.length === 0) {
      this.t.diagnostic(`no ${SERVER_LOG} under ${logs}`);
    }
    for (const entry of serverLogs) {
      this.attach(join(logs, entry));
    }
  }

  /** Reports the file at `path` as diagnostics, one per line. */
  private attach(path: string): void {
    this.t.diagnostic(`${path}:`);
    for (const line of readFileSync(path, "utf8").trimEnd().split("\n")) {
      this.t.diagnostic(`  ${line}`);
    }
  }
}

function shellQuote(word: string): string {
  return `'${word.replaceAll("'", `'\\''`)}'`;
}

test(
  "starts the cairn-lsp found on PATH with no arguments",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    await fixture.expectLaunches({ user: {} }, [
      probe(server),
      probe(server),
      session(server, [UNPINNED]),
    ]);
  },
);

test(
  "starts cairn.serverPath in place of the cairn-lsp on PATH",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.server("configured");
    await fixture.expectLaunches({ user: { "cairn.serverPath": server } }, [
      probe(server),
      probe(server),
      session(server, [UNPINNED]),
    ]);
  },
);

/** The settings JSON at `path`. */
function settingsAt(path: string): unknown {
  return JSON.parse(readFileSync(path, "utf8"));
}

test(
  "sends the pin the settings name, and the target cairn.setTarget writes where it is set",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    await fixture.expectLaunches(
      {
        user: { "cairn.edition": "java", "cairn.target": "1.21" },
        setTarget: ["java", "1.21.4"],
      },
      [
        probe(server),
        probe(server),
        session(server, [
          { edition: "java", target: "1.21" },
          { edition: "java", target: "1.21.4" },
        ]),
      ],
    );
    assert.deepEqual(settingsAt(fixture.userSettings), {
      "cairn.edition": "java",
      "cairn.target": "1.21.4",
    });
  },
);

test(
  "writes the target to the workspace that sets cairn.target, not beside a user edition",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    await fixture.expectLaunches(
      {
        user: { "cairn.edition": "java" },
        workspace: { "cairn.target": "1.21" },
        setTarget: ["java", "1.21.4"],
      },
      [
        probe(server),
        probe(server),
        session(server, [
          { edition: "java", target: "1.21" },
          { edition: "java", target: "1.21.4" },
        ]),
      ],
    );
    assert.deepEqual(settingsAt(fixture.userSettings), {
      "cairn.edition": "java",
    });
    assert.deepEqual(settingsAt(fixture.workspaceSettings), {
      "cairn.target": "1.21.4",
    });
  },
);

test(
  "writes a target no scope sets beside the workspace's cairn.edition",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    await fixture.expectLaunches(
      {
        user: {},
        workspace: { "cairn.edition": "java" },
        setTarget: ["java", "1.21.4"],
      },
      [
        probe(server),
        probe(server),
        session(server, [
          { edition: "java", target: "" },
          { edition: "java", target: "1.21.4" },
        ]),
      ],
    );
    assert.deepEqual(settingsAt(fixture.userSettings), {});
    assert.deepEqual(settingsAt(fixture.workspaceSettings), {
      "cairn.edition": "java",
      "cairn.target": "1.21.4",
    });
  },
);

test(
  "writes a target no scope sets beside a user cairn.edition, though a folder is open",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    await fixture.expectLaunches(
      {
        user: { "cairn.edition": "java" },
        workspace: {},
        setTarget: ["java", "1.21.4"],
      },
      [
        probe(server),
        probe(server),
        session(server, [
          { edition: "java", target: "" },
          { edition: "java", target: "1.21.4" },
        ]),
      ],
    );
    assert.deepEqual(settingsAt(fixture.userSettings), {
      "cairn.edition": "java",
      "cairn.target": "1.21.4",
    });
    assert.deepEqual(settingsAt(fixture.workspaceSettings), {});
  },
);

test(
  "refuses a target of an edition cairn.edition no longer names",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    const user = { "cairn.edition": "java", "cairn.target": "1.21" };
    await fixture.expectLaunches(
      { user, setTarget: ["bedrock", "1.21.60"] },
      [
        probe(server),
        probe(server),
        session(server, [{ edition: "java", target: "1.21" }]),
      ],
    );
    assert.deepEqual(settingsAt(fixture.userSettings), user);
  },
);

test(
  "does not fall back to PATH when cairn.serverPath cannot be launched",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const missing = join(fixture.root, "missing", "cairn-lsp");
    await fixture.expectLaunches({ user: { "cairn.serverPath": missing } }, []);
  },
);

test(
  "Cairn: Show Info opens what cairn.info answers for the document",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    const shown = join(fixture.root, "shown.json");
    await fixture.expectLaunches({ user: {}, showInfo: shown }, [
      probe(server),
      probe(server),
      {
        ...session(server, [UNPINNED]),
        received: [
          "initialize",
          "initialized",
          "workspace/executeCommand",
          "shutdown",
          "exit",
        ],
      },
    ]);
    const request = {
      command: "cairn.info",
      arguments: [{ uri: pathToFileURL(fixture.document).href }],
    };
    assert.equal(
      readFileSync(shown, "utf8"),
      `${JSON.stringify(request, null, 2)}\n`,
    );
  },
);

test(
  "Cairn: Diff Blocks opens what cairn.diffBlocks answers for the document and a structure file",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.pathServer;
    const shown = join(fixture.root, "shown.json");
    const structure = join(fixture.root, "main.nbt");
    writeFileSync(structure, "");
    await fixture.expectLaunches(
      { user: {}, diffBlocks: { shown, structure } },
      [
        probe(server),
        probe(server),
        {
          ...session(server, [UNPINNED]),
          received: [
            "initialize",
            "initialized",
            "workspace/executeCommand",
            "shutdown",
            "exit",
          ],
        },
      ],
    );
    const request = {
      command: "cairn.diffBlocks",
      arguments: [
        {
          uri: pathToFileURL(fixture.document).href,
          structure: pathToFileURL(structure).href,
        },
      ],
    };
    assert.equal(
      readFileSync(shown, "utf8"),
      `${JSON.stringify(request, null, 2)}\n`,
    );
  },
);
