import assert from "node:assert/strict";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { test, type TestContext } from "node:test";
import { setTimeout } from "node:timers/promises";
import { runTests } from "@vscode/test-electron";
import type { ServerEvent } from "./fake-cairn-lsp";

const EXTENSION_ROOT = resolve(__dirname, "..", "..");
const FAKE_SERVER = join(__dirname, "fake-cairn-lsp.js");
const EXIT_TIMEOUT_MS = 10_000;
const LIFECYCLE = ["initialize", "initialized", "shutdown", "exit"];
const POSIX_ONLY = {
  skip:
    process.platform === "win32" &&
    "the stand-in cairn-lsp is a POSIX shell script",
};

/**
 * One run of a stand-in `cairn-lsp`: the path it was started as, its
 * arguments, and the lifecycle messages it handled.
 */
interface Launch {
  readonly server: string;
  readonly argv: string[];
  readonly received: string[];
}

/**
 * A temporary directory holding a `.crn` document, the record, and a
 * stand-in `cairn-lsp` for `PATH`.
 */
class Fixture {
  readonly root = mkdtempSync(join(tmpdir(), "cairn-vscode-"));
  readonly document = join(this.root, "main.crn");
  readonly record = join(this.root, "launches.jsonl");
  readonly pathServer = this.server("path");

  constructor(t: TestContext) {
    t.after(() => rmSync(this.root, { recursive: true, force: true }));
    writeFileSync(this.document, "");
    writeFileSync(this.record, "");
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

  /** Opens the document in VS Code with the extension and user `settings`. */
  async open(settings: Record<string, unknown>): Promise<void> {
    const userData = join(this.root, "user-data");
    mkdirSync(join(userData, "User"), { recursive: true });
    const settingsFile = join(userData, "User", "settings.json");
    writeFileSync(settingsFile, JSON.stringify(settings));
    await runTests({
      extensionDevelopmentPath: EXTENSION_ROOT,
      extensionTestsPath: join(__dirname, "host.js"),
      launchArgs: [
        this.document,
        "--disable-extensions",
        // Otherwise VS Code takes `PATH` from a login shell, whose profile
        // could put an installed `cairn-lsp` ahead of the stand-in.
        "--force-disable-user-env",
        `--user-data-dir=${userData}`,
        `--extensions-dir=${join(this.root, "extensions")}`,
      ],
      extensionTestsEnv: {
        PATH: [dirname(this.pathServer), process.env.PATH].join(delimiter),
      },
    });
  }

  /**
   * The stand-in launches in the order they started, once each has exited or
   * the wait has run out.
   */
  async launches(): Promise<Launch[]> {
    const deadline = Date.now() + EXIT_TIMEOUT_MS;
    for (;;) {
      const launches = new Map<string, Launch>();
      const running = new Set<string>();
      for (const line of readFileSync(this.record, "utf8").split("\n")) {
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
        } else {
          running.delete(event.id);
        }
      }
      if (running.size === 0 || Date.now() > deadline) {
        return [...launches.values()];
      }
      await setTimeout(100);
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
    await fixture.open({});
    assert.deepEqual(await fixture.launches(), [
      { server: fixture.pathServer, argv: ["--version"], received: [] },
      { server: fixture.pathServer, argv: ["--version"], received: [] },
      { server: fixture.pathServer, argv: [], received: LIFECYCLE },
    ]);
  },
);

test(
  "starts cairn.serverPath in place of the cairn-lsp on PATH",
  POSIX_ONLY,
  async (t) => {
    const fixture = new Fixture(t);
    const configured = fixture.server("configured");
    await fixture.open({ "cairn.serverPath": configured });
    assert.deepEqual(await fixture.launches(), [
      { server: configured, argv: ["--version"], received: [] },
      { server: configured, argv: ["--version"], received: [] },
      { server: configured, argv: [], received: LIFECYCLE },
    ]);
  },
);
