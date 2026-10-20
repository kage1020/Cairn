import { join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { extensions } from "vscode";

const EXTENSION_ID = "kage1020.cairn-lang";
const ACTIVATION_TIMEOUT_MS = 30_000;

/**
 * Waits for the `.crn` document the window opened with to activate the
 * extension, then deactivates it.
 */
export async function run(): Promise<void> {
  const extension = extensions.getExtension(EXTENSION_ID);
  if (!extension) {
    throw new Error(`${EXTENSION_ID} is not loaded in the test instance`);
  }
  const deadline = Date.now() + ACTIVATION_TIMEOUT_MS;
  while (!extension.isActive) {
    if (Date.now() > deadline) {
      throw new Error(`${EXTENSION_ID} did not activate on the .crn document`);
    }
    await setTimeout(100);
  }
  const main = join(extension.extensionPath, extension.packageJSON.main);
  // VS Code's own call at shutdown races the window's exit, which could cut
  // the record off at any message.
  const { deactivate } = require(main) as typeof import("../extension");
  await deactivate();
}
