/** The editor's colouring: the VS Code extension's TextMate grammar, with each finding underlined. */

import type { HighlighterCore, LanguageRegistration, ThemedToken } from "@shikijs/core";
import grammar from "../../../editors/vscode/syntaxes/cairn.tmLanguage.json";
import type { Severity } from "./protocol";

/** A stretch of the source a finding is about, in UTF-16 offsets; it may be empty. */
export interface Mark {
  start: number;
  end: number;
  severity: Severity;
}

let highlighter: Promise<HighlighterCore> | undefined;

/** The highlighter, loaded on first use. */
export function loadHighlighter(): Promise<HighlighterCore> {
  highlighter ??= (async () => {
    const [{ createHighlighterCore }, { createJavaScriptRegexEngine }] = await Promise.all([
      import("@shikijs/core"),
      import("@shikijs/engine-javascript"),
    ]);
    return createHighlighterCore({
      themes: [import("@shikijs/themes/github-light"), import("@shikijs/themes/github-dark")],
      langs: [{ ...(grammar as unknown as LanguageRegistration), name: "cairn" }],
      engine: createJavaScriptRegexEngine(),
    });
  })();
  return highlighter;
}

/** `source` as coloured spans in `into`, each mark's stretch underlined by its severity. */
export function render(
  into: HTMLElement,
  source: string,
  marks: Mark[],
  highlighter: HighlighterCore | undefined,
): void {
  const lines: ThemedToken[][] = highlighter
    ? highlighter.codeToTokens(source, {
        lang: "cairn",
        themes: { light: "github-light", dark: "github-dark" },
        defaultColor: false,
      }).tokens
    : plainLines(source);
  const rankAt = ranks(source.length, marks);
  const fragment = document.createDocumentFragment();
  let lineEnd = -1;
  lines.forEach((tokens, index) => {
    if (index > 0) fragment.append("\n");
    for (const token of tokens) appendToken(fragment, token, rankAt);
    lineEnd = source.indexOf("\n", lineEnd + 1);
    if (lineEnd < 0) lineEnd = source.length;
    // The newline is not drawn, so a mark on it, such as a missing value's, gets a cell.
    if (rankAt[lineEnd]) fragment.append(span(" ", rankAt[lineEnd]));
  });
  // A trailing newline in the textarea is a line the overlay must also have.
  fragment.append("\n ");
  into.replaceChildren(fragment);
}

function plainLines(source: string): ThemedToken[][] {
  let offset = 0;
  return source.split("\n").map((content) => {
    const token = { content, offset } as ThemedToken;
    offset += content.length + 1;
    return content ? [token] : [];
  });
}

/** Per UTF-16 unit, and one past the end, the most severe mark: 0 none, 1 warning, 2 error. */
function ranks(length: number, marks: Mark[]): Uint8Array {
  const at = new Uint8Array(length + 1);
  for (const mark of marks) {
    const rank = mark.severity === "error" ? 2 : 1;
    const end = Math.min(Math.max(mark.end, mark.start + 1), length + 1);
    for (let i = Math.max(mark.start, 0); i < end; i += 1) at[i] = Math.max(at[i], rank);
  }
  return at;
}

function span(text: string, rank: number): HTMLSpanElement {
  const element = document.createElement("span");
  element.textContent = text;
  if (rank) element.className = rank === 2 ? "cp-mark-error" : "cp-mark-warning";
  return element;
}

function appendToken(into: DocumentFragment, token: ThemedToken, rankAt: Uint8Array): void {
  const { content, offset } = token;
  let from = 0;
  while (from < content.length) {
    const rank = rankAt[offset + from];
    let to = from + 1;
    while (to < content.length && rankAt[offset + to] === rank) to += 1;
    const piece = span(content.slice(from, to), rank);
    for (const [name, value] of Object.entries(token.htmlStyle ?? {})) {
      piece.style.setProperty(name, value);
    }
    into.append(piece);
    from = to;
  }
}
