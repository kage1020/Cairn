import { join } from "node:path";
import { setTimeout } from "node:timers/promises";
import { extensions } from "vscode";

const EXTENSION_ID = "kage1020.cairn-lang";
const ACTIVATION_TIMEOUT_MS = 30_000;

/**
 * Waits for the `.crn` document the window opened with to activate the
 * extension, then stops its language client through `deactivate`.
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
