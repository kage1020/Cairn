/** The playground: an editor whose source the worker compiles as it changes, and what it built. */

import type { HighlighterCore } from "@shikijs/core";
import { type Mark, loadHighlighter, render } from "./highlight";
import type {
  BlockArray,
  Compiled,
  Diagnostic,
  Request,
  Response,
  RunLevelMessage,
  Targets,
} from "./protocol";
import { type Strings, count, fill } from "./strings";
import { colourOf, counts, draw } from "./view";

const examples = import.meta.glob<string>("../../../examples/*.crn", {
  query: "?raw",
  import: "default",
  eager: true,
});

const EDITIONS = ["java", "bedrock"] as const;
const FIRST_EXAMPLE = "cottage";
const SETTLE_MS = 250;

type Pending = { resolve: (value: Compiled | Targets) => void; reject: (error: Error) => void };

/** A request before it is numbered. */
type Unnumbered<T> = T extends unknown ? Omit<T, "id"> : never;

class Playground extends HTMLElement {
  private t!: Strings;
  private input!: HTMLTextAreaElement;
  private code!: HTMLElement;
  private example!: HTMLSelectElement;
  private edition!: HTMLSelectElement;
  private target!: HTMLSelectElement;
  private status!: HTMLElement;
  private findings!: HTMLElement;
  private summary!: HTMLElement;
  private structure!: HTMLSelectElement;
  private layers!: HTMLInputElement;
  private layersValue!: HTMLOutputElement;
  private canvas!: HTMLCanvasElement;
  private legend!: HTMLElement;
  private ir!: HTMLDetailsElement;
  private irText!: HTMLElement;
  private downloads!: HTMLElement;
  private viewer!: HTMLElement;
  private emptyView!: HTMLElement;

  private worker: Worker | undefined;
  private requests = new Map<number, Pending>();
  private nextId = 0;
  private version = "";
  private highlighter: HighlighterCore | undefined;
  private marks: Mark[] = [];
  private compiled: Compiled | undefined;
  private turns = 0;
  private settle: number | undefined;
  private generation = 0;
  private urls: string[] = [];
  private leaveOnTab = false;
  private restarted = false;

  connectedCallback(): void {
    this.t = JSON.parse(this.dataset.strings ?? "{}") as Strings;
    const find = <T extends HTMLElement>(role: string): T => {
      const element = this.querySelector<T>(`[data-role="${role}"]`);
      if (!element) throw new Error(`the playground has no \`${role}\``);
      return element;
    };
    this.input = find("input");
    this.code = find("code");
    this.example = find("example");
    this.edition = find("edition");
    this.target = find("target");
    this.status = find("status");
    this.findings = find("findings");
    this.summary = find("summary");
    this.structure = find("structure");
    this.layers = find("layers");
    this.layersValue = find("layers-value");
    this.canvas = find("canvas");
    this.legend = find("legend");
    this.ir = find("ir");
    this.irText = find("ir-text");
    this.downloads = find("downloads");
    this.viewer = find("viewer");
    this.emptyView = find("empty-view");

    const names = Object.keys(examples)
      .map((path) => path.replace(/^.*\//, "").replace(/\.crn$/, ""))
      .sort();
    for (const name of names) this.example.append(new Option(name, name, false, name === FIRST_EXAMPLE));
    for (const edition of EDITIONS) this.edition.append(new Option(edition, edition));
    this.input.value = this.exampleSource(this.example.value);

    this.input.addEventListener("input", () => this.edited());
    this.input.addEventListener("scroll", () => this.followScroll());
    this.input.addEventListener("keydown", (event) => this.keyDown(event));
    this.example.addEventListener("change", () => {
      this.input.value = this.exampleSource(this.example.value);
      this.input.scrollTop = 0;
      this.edited();
    });
    this.edition.addEventListener("change", () => void this.editionChanged());
    this.target.addEventListener("change", () => this.compileNow());
    this.structure.addEventListener("change", () => this.structureChanged());
    this.layers.addEventListener("input", () => this.redraw());
    find("rotate").addEventListener("click", () => {
      this.turns += 1;
      this.redraw();
    });
    this.ir.addEventListener("toggle", () => this.fillIr());
    new ResizeObserver(() => this.redraw()).observe(this.canvas);
    new MutationObserver(() => this.redraw()).observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });

    this.paint();
    void loadHighlighter().then((highlighter) => {
      this.highlighter = highlighter;
      this.paint();
    });
    this.status.textContent = this.t.loading;
    this.spawn();
  }

  private exampleSource(name: string): string {
    return examples[`../../../examples/${name}.crn`] ?? "";
  }

  private spawn(): void {
    this.worker?.terminate();
    for (const pending of this.requests.values()) pending.reject(new Error("restarted"));
    this.requests.clear();
    const worker = new Worker(new URL("./worker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (event: MessageEvent<Response>) => this.answered(event.data);
    this.worker = worker;
  }

  private answered(message: Response): void {
    if (message.kind === "ready") {
      this.version = message.version;
      // After a crash, compiling the same source again would only crash again; the next edit will.
      if (this.restarted) this.restarted = false;
      else void this.editionChanged();
      return;
    }
    if (message.kind === "unavailable") {
      this.unavailable();
      return;
    }
    const pending = this.requests.get(message.id);
    this.requests.delete(message.id);
    if (message.kind === "answer") {
      pending?.resolve(message.value);
    } else {
      pending?.reject(new Error(message.message));
      if (message.trapped) {
        this.status.textContent = this.t.crashed;
        this.restarted = true;
        this.spawn();
      }
    }
  }

  private ask(request: Unnumbered<Request>): Promise<Compiled | Targets> {
    const id = this.nextId++;
    return new Promise((resolve, reject) => {
      this.requests.set(id, { resolve, reject });
      this.worker?.postMessage({ ...request, id });
    });
  }

  private unavailable(): void {
    this.status.replaceChildren(this.t.unavailable, " ");
    const link = document.createElement("a");
    link.href = "/development/#the-playgrounds-module";
    link.textContent = this.t.unavailableLink;
    this.status.append(link);
    for (const control of [this.edition, this.target]) control.disabled = true;
  }

  private async editionChanged(): Promise<void> {
    const targets = (await this.ask({ kind: "targets", edition: this.edition.value })) as Targets;
    this.target.replaceChildren(
      new Option(fill(this.t.latest, { version: targets.latest }), "latest"),
      ...targets.versions
        .slice()
        .reverse()
        .map((version) => new Option(version, version)),
    );
    this.compileNow();
  }

  private edited(): void {
    this.paint();
    window.clearTimeout(this.settle);
    this.settle = window.setTimeout(() => this.compileNow(), SETTLE_MS);
  }

  private compileNow(): void {
    if (!this.version) return;
    window.clearTimeout(this.settle);
    const generation = ++this.generation;
    const source = this.input.value;
    this.status.textContent = this.t.compiling;
    this.ask({
      kind: "compile",
      source,
      edition: this.edition.value,
      target: this.target.value || "latest",
    }).then(
      (value) => {
        if (generation === this.generation) this.show(value as Compiled, source);
      },
      () => undefined,
    );
  }

  private show(compiled: Compiled, source: string): void {
    this.compiled = compiled;
    this.status.textContent = compiled.ok
      ? fill(this.t.built, { version: this.version })
      : this.t.refused;
    this.marks = [
      ...compiled.diagnostics.map((d) => ({
        start: offsetOf(source, d.line, d.col),
        end: offsetOf(source, d.end_line, d.end_col),
        severity: d.severity,
      })),
      ...compiled.run_level
        .filter((m) => m.line !== undefined && m.col !== undefined)
        .map((m) => {
          const start = offsetOf(source, m.line ?? 1, m.col ?? 1);
          return { start, end: start, severity: m.severity };
        }),
    ];
    this.paint();
    this.listFindings(compiled, source);
    this.showResult(compiled);
    this.offerDownloads(compiled);
  }

  private paint(): void {
    render(this.code, this.input.value, this.marks, this.highlighter);
    this.followScroll();
  }

  private followScroll(): void {
    this.code.scrollTop = this.input.scrollTop;
    this.code.scrollLeft = this.input.scrollLeft;
  }

  private keyDown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      this.leaveOnTab = true;
      return;
    }
    const leave = this.leaveOnTab;
    this.leaveOnTab = false;
    if (event.key === "Tab" && !event.shiftKey && !event.altKey && !event.ctrlKey && !leave) {
      event.preventDefault();
      insert(this.input, "  ");
    } else if (event.key === "Enter" && !event.shiftKey && !event.altKey && !event.ctrlKey) {
      // Cairn nests by indentation, so a new line starts where the one above it did.
      const { value, selectionStart } = this.input;
      const lineStart = value.lastIndexOf("\n", selectionStart - 1) + 1;
      const indent = /^ */.exec(value.slice(lineStart, selectionStart))?.[0] ?? "";
      event.preventDefault();
      insert(this.input, `\n${indent}`);
    }
  }

  private listFindings(compiled: Compiled, source: string): void {
    const all: (Diagnostic | RunLevelMessage)[] = [...compiled.diagnostics, ...compiled.run_level];
    const errors = all.filter((d) => d.severity === "error").length;
    const warnings = all.length - errors;
    this.summary.textContent = all.length
      ? [count(this.t.errors, errors), count(this.t.warnings, warnings)].join(this.t.and)
      : this.t.noFindings;
    this.findings.replaceChildren(
      ...all.map((finding) => {
        const item = document.createElement("li");
        item.className = `cp-finding cp-finding-${finding.severity}`;
        const head = document.createElement(finding.line === undefined ? "span" : "button");
        head.className = "cp-finding-head";
        const where = finding.line === undefined ? "" : ` ${finding.line}:${finding.col}`;
        const code = finding.code ? `[${finding.code}]` : "";
        head.textContent = `${finding.severity}${code}${where}: ${finding.primary}`;
        if (head instanceof HTMLButtonElement) {
          head.type = "button";
          head.addEventListener("click", () => {
            const offset = offsetOf(source, finding.line ?? 1, finding.col ?? 1);
            this.input.focus();
            this.input.setSelectionRange(offset, offset);
          });
        }
        item.append(head);
        if (finding.notes?.length) {
          const notes = document.createElement("ul");
          for (const note of finding.notes) {
            const line = document.createElement("li");
            line.textContent =
              note.line === undefined ? note.message : `${note.line}:${note.col}: ${note.message}`;
            notes.append(line);
          }
          item.append(notes);
        }
        return item;
      }),
    );
  }

  private showResult(compiled: Compiled): void {
    const keys = compiled.ir ? Object.keys(compiled.ir.structures) : [];
    this.viewer.hidden = keys.length === 0;
    this.emptyView.hidden = keys.length > 0;
    const kept = keys.includes(this.structure.value) ? this.structure.value : keys[0];
    this.structure.replaceChildren(...keys.map((key) => new Option(key, key, false, key === kept)));
    this.structure.disabled = keys.length < 2;
    this.structureChanged(true);
    if (this.ir.open) this.fillIr();
  }

  private current(): BlockArray | undefined {
    return this.compiled?.ir?.structures[this.structure.value];
  }

  private structureChanged(keepLayers = false): void {
    const array = this.current();
    if (!array) return;
    const all = this.layers.value === this.layers.max;
    this.layers.max = String(array.dims.y);
    if (!keepLayers || all || Number(this.layers.value) > array.dims.y) {
      this.layers.value = String(array.dims.y);
    }
    this.listPalette(array);
    this.redraw();
  }

  private redraw(): void {
    const array = this.current();
    if (!array || this.viewer.hidden) return;
    const layers = Number(this.layers.value);
    this.layersValue.value = `${layers} / ${array.dims.y}`;
    const background = getComputedStyle(this.canvas).getPropertyValue("--cp-canvas").trim();
    draw(this.canvas, { array, layers, turns: this.turns }, background || "transparent");
    const { x, y, z } = array.dims;
    this.canvas.setAttribute(
      "aria-label",
      `${this.structure.value}: ${x} × ${y} × ${z}, ${this.layersValue.value}`,
    );
  }

  private listPalette(array: BlockArray): void {
    const tally = counts(array);
    this.legend.replaceChildren(
      ...array.palette.flatMap((state, index) => {
        if (index === 0 || !tally[index]) return [];
        const item = document.createElement("li");
        const swatch = document.createElement("span");
        swatch.className = "cp-swatch";
        swatch.style.background = colourOf(state);
        const properties = Object.entries(state.properties ?? {})
          .map(([name, value]) => `${name}=${value}`)
          .join(",");
        const label = document.createElement("code");
        label.textContent = properties ? `${state.id}[${properties}]` : state.id;
        item.append(swatch, label, `× ${tally[index]}`);
        return [item];
      }),
    );
  }

  private fillIr(): void {
    if (!this.ir.open) return;
    this.irText.textContent = this.compiled?.ir ? JSON.stringify(this.compiled.ir, null, 2) : "";
  }

  private offerDownloads(compiled: Compiled): void {
    for (const url of this.urls) URL.revokeObjectURL(url);
    this.urls = [];
    if (!compiled.artifacts.length) {
      const none = document.createElement("p");
      none.textContent = this.t.noDownloads;
      this.downloads.replaceChildren(none);
      return;
    }
    const list = document.createElement("ul");
    for (const artifact of compiled.artifacts) {
      const url = URL.createObjectURL(
        new Blob([artifact.bytes as BlobPart], { type: "application/octet-stream" }),
      );
      this.urls.push(url);
      const link = document.createElement("a");
      link.href = url;
      link.download = artifact.name;
      link.textContent = artifact.name;
      const item = document.createElement("li");
      item.append(link, ` (${artifact.bytes.length.toLocaleString()} B)`);
      list.append(item);
    }
    this.downloads.replaceChildren(list);
  }
}

/** The UTF-16 offset of 1-based `line` and `col`, a column counted in Unicode scalar values. */
function offsetOf(source: string, line: number, col: number): number {
  let offset = 0;
  for (let at = 1; at < line; at += 1) {
    const next = source.indexOf("\n", offset);
    if (next < 0) return source.length;
    offset = next + 1;
  }
  for (let at = 1; at < col && offset < source.length && source[offset] !== "\n"; at += 1) {
    offset += (source.codePointAt(offset) ?? 0) > 0xffff ? 2 : 1;
  }
  return offset;
}

/** Type `text` over the selection, keeping the browser's undo history where it can. */
function insert(input: HTMLTextAreaElement, text: string): void {
  if (!document.execCommand("insertText", false, text)) {
    input.setRangeText(text, input.selectionStart, input.selectionEnd, "end");
    input.dispatchEvent(new Event("input"));
  }
}

customElements.define("cairn-playground", Playground);
