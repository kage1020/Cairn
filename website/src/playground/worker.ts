/** Runs `cairn-lang-wasm` off the page's thread, so a long build never freezes the editor. */

import type { Compiled, Edition, Request, Response, Targets } from "./protocol";

interface Bindings {
  default: () => Promise<unknown>;
  cairn_version(): string;
  compile(source: string, edition: Edition, target: string): Compiled;
  targets(edition: Edition): Targets;
}

const scope = self as unknown as {
  onmessage: ((event: MessageEvent<Request>) => void) | null;
  postMessage(message: Response, transfer?: Transferable[]): void;
};

// A glob, not an import: `pnpm run build:wasm` writes the module, and a site built without it
// still has to build.
const modules = import.meta.glob<Bindings>("./pkg/cairn_lang_wasm.js");
const load = modules["./pkg/cairn_lang_wasm.js"];

const bindings: Promise<Bindings | undefined> = load
  ? load().then(async (module) => {
      await module.default();
      return module;
    })
  : Promise.resolve(undefined);

bindings.then(
  (module) =>
    scope.postMessage(
      module ? { kind: "ready", version: module.cairn_version() } : { kind: "unavailable" },
    ),
  (error: unknown) => scope.postMessage({ kind: "load-failed", message: String(error) }),
);

scope.onmessage = async (event) => {
  const request = event.data;
  // A module that failed to load has said so, and answers nothing.
  const module = await bindings.catch(() => undefined);
  if (!module) return;
  try {
    if (request.kind === "compile") {
      const value = module.compile(request.source, request.edition, request.target);
      const transfer = value.artifacts.map((artifact) => artifact.bytes.buffer);
      scope.postMessage({ kind: "answer", id: request.id, value }, transfer);
    } else {
      scope.postMessage({ kind: "answer", id: request.id, value: module.targets(request.edition) });
    }
  } catch (error) {
    // A Rust panic aborts, which traps; the instance cannot be trusted after that.
    const trapped = error instanceof WebAssembly.RuntimeError;
    scope.postMessage({ kind: "failed", id: request.id, message: String(error), trapped });
  }
};
