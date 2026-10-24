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
import { test, type TestContext } from "node:test";
import { setTimeout } from "node:timers/promises";
import { runTests } from "@vscode/test-electron";
import type { ServerEvent } from "./server-event";

const EXTENSION_ROOT = resolve(__dirname, "..", "..");
const FAKE_SERVER = join(__dirname, "fake-cairn-lsp.js");
// CI's `.vscode-test` cache key is read from this line.
const VSCODE_VERSION = "1.143.1";
const EXIT_TIMEOUT_MS = 10_000;
const SERVER_LOG = "Cairn Language Server.log";
const LIFECYCLE = ["initialize", "initialized", "shutdown", "exit"];
const POSIX_ONLY = {
  skip:
    process.platform === "win32" &&
    "the stand-in cairn-lsp is a POSIX shell script",
};

/**
 * One run of a stand-in `cairn-lsp`: the path the spawn resolved, its
 * arguments, the lifecycle messages it received, and its exit code.
 */
interface Launch {
  readonly server: string;
  readonly argv: string[];
  readonly received: string[];
  exitCode?: number;
}

/**
 * A temporary directory holding a `.crn` document, the record the stand-ins
 * append to, and a stand-in `cairn-lsp` for `PATH`.
 */
class Fixture {
  readonly root = mkdtempSync(join(tmpdir(), "cairn-vscode-"));
  readonly document = join(this.root, "main.crn");
  readonly record = join(this.root, "launches.jsonl");
  readonly userData = join(this.root, "user-data");
  readonly pathServer: string;
  private readonly t: TestContext;

  constructor(t: TestContext) {
    this.t = t;
    t.after(() => rmSync(this.root, { recursive: true, force: true }));
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
   * Runs VS Code with user `settings` and asserts the stand-ins' launches
   * equal `expected`, reporting the record and the server log if not.
   */
  async expectLaunches(
    settings: Record<string, unknown>,
    expected: Launch[],
  ): Promise<void> {
    const failure = await this.open(settings).then(
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

  /**
   * Runs VS Code on the document with the extension and user `settings`
   * until it exits.
   */
  private async open(settings: Record<string, unknown>): Promise<void> {
    mkdirSync(join(this.userData, "User"), { recursive: true });
    const settingsFile = join(this.userData, "User", "settings.json");
    writeFileSync(settingsFile, JSON.stringify(settings));
    await runTests({
      version: VSCODE_VERSION,
      extensionDevelopmentPath: EXTENSION_ROOT,
      extensionTestsPath: join(__dirname, "host.js"),
      launchArgs: [
        this.document,
        "--disable-extensions",
        // Otherwise VS Code takes `PATH` from a login shell, whose profile
        // could put an installed `cairn-lsp` ahead of the stand-in.
        "--force-disable-user-env",
        `--user-data-dir=${this.userData}`,
        `--extensions-dir=${join(this.root, "extensions")}`,
      ],
      extensionTestsEnv: {
        PATH: [dirname(this.pathServer), process.env.PATH].join(delimiter),
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
          launches.set(event.id, { server, argv, received: [] });
          running.add(event.id);
        } else if ("received" in event) {
          launches.get(event.id)?.received.push(event.received);
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
    await fixture.expectLaunches({}, [
      { server, argv: ["--version"], received: [], exitCode: 0 },
      { server, argv: ["--version"], received: [], exitCode: 0 },
      { server, argv: [], received: LIFECYCLE, exitCode: 0 },
    ]);
  },
);

test(
  "starts cairn.serverPath in place of the cairn-lsp on PATH",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const server = fixture.server("configured");
    await fixture.expectLaunches({ "cairn.serverPath": server }, [
      { server, argv: ["--version"], received: [], exitCode: 0 },
      { server, argv: ["--version"], received: [], exitCode: 0 },
      { server, argv: [], received: LIFECYCLE, exitCode: 0 },
    ]);
  },
);

test(
  "does not fall back to PATH when cairn.serverPath cannot be launched",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const missing = join(fixture.root, "missing", "cairn-lsp");
    await fixture.expectLaunches({ "cairn.serverPath": missing }, []);
  },
);
