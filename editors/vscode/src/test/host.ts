import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { type TextDocument, commands, extensions, workspace } from "vscode";
import type { ServerEvent } from "./server-event";

const EXTENSION_ID = "kage1020.cairn-lang";
const ACTIVATION_TIMEOUT_MS = 30_000;
const PIN_TIMEOUT_MS = 30_000;

/**
 * Waits for the `.crn` document the window opened with to activate the
 * extension, runs `cairn.setTarget` with the edition and version
 * `CAIRN_TEST_SET_TARGET` holds as JSON, runs `Cairn: Show Info` when
 * `CAIRN_TEST_SHOW_INFO` names a file for what it shows, then stops its
 * language client through `deactivate`.
 */
export async function run(): Promise<void> {
  const extension = extensions.getExtension(EXTENSION_ID);
  if (!extension) {
    throw new Error(`${EXTENSION_ID} is not loaded in the test instance`);
  }
  const deadline = Date.now() + ACTIVATION_TIMEOUT_MS;
  while (!extension.isActive) {
    if (Date.now() > deadline) {
      throw new Error(
        `${EXTENSION_ID}: activate did not finish within ${ACTIVATION_TIMEOUT_MS / 1_000} s`,
      );
    }
    await setTimeout(100);
  }
  const asked = process.env.CAIRN_TEST_SET_TARGET;
  if (asked) {
    const [edition, version] = JSON.parse(asked) as [string, string];
    await commands.executeCommand("cairn.setTarget", edition, version);
    // A refused command writes nothing, so no new pin is coming.
    if (workspace.getConfiguration("cairn").get("target") === version) {
      await pinReached(version);
    }
  }
  const shownTo = process.env.CAIRN_TEST_SHOW_INFO;
  if (shownTo) {
    const shown = await commands.executeCommand<TextDocument | undefined>(
      "cairn.showInfo",
    );
    writeFileSync(shownTo, shown?.getText() ?? "");
  }
  const main = join(extension.extensionPath, extension.packageJSON.main);
  const loaded = require.cache[require.resolve(main)];
  if (!loaded) {
    throw new Error(`${main} is not the module VS Code loaded`);
  }
  // VS Code's own deactivate at shutdown races the window's exit, which can
  // kill the stand-in before it records shutdown and exit.
  const { deactivate } = loaded.exports as typeof import("../extension");
  await deactivate();
}

/**
 * Waits until a stand-in records a configuration change that pins `target`,
 * so the client is not stopped before the change reaches it.
 */
async function pinReached(target: string): Promise<void> {
  const record = process.env.CAIRN_TEST_RECORD;
  if (!record) {
    throw new Error("CAIRN_TEST_RECORD names no record to read the pin from");
  }
  const deadline = Date.now() + PIN_TIMEOUT_MS;
  for (;;) {
    const reached = readFileSync(record, "utf8")
      .split("\n")
      .filter((line) => line.length > 0)
      .some((line) => {
        const event = JSON.parse(line) as ServerEvent;
        return "pin" in event && event.pin.target === target;
      });
    if (reached) {
      return;
    }
    if (Date.now() > deadline) {
      throw new Error(
        `no stand-in received a pin of ${target} within ${PIN_TIMEOUT_MS / 1_000} s`,
      );
    }
    await setTimeout(100);
  }
}
