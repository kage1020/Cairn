# cairn-lang-lsp

Language Server Protocol implementation for Cairn editors. It puts the parser and lint diagnostics from [`cairn-lang-core`](../cairn-lang-core/README.md) in the margin as you type, completes material tokens and describes what the cursor rests on, so the write-check-fix loop described in [lint](https://cairn.kage1020.com/spec/lint/) happens in the editor rather than in a terminal. The [VS Code extension](../../editors/vscode/README.md) is a thin client over this binary.

## Status

The `cairn-lsp` binary speaks standard LSP over stdio: it negotiates full-content document sync at `initialize`, reports diagnostics — what a `cairn check` with no `--edition` reports: the same `parse → lower → check` passes and the lowering-stage findings every target that reads their line raises, without the `@intended_targets` findings the CLI merges in — and answers `textDocument/completion` from the closed vocabularies. A client whose `initialize` declares `textDocument.diagnostic` pulls them with `textDocument/diagnostic` and is never published to; any other client is pushed `textDocument/publishDiagnostics` on every `didOpen`/`didChange`, cleared on `didClose`. Diagnostics are computed on a second thread, which answers a pull before it publishes and skips a revision a newer one has replaced, so a completion request is not kept waiting behind them. It answers `textDocument/hover` from the stored text. Code actions are not yet wired.

## Shipped capabilities

| Capability | Notes |
|---|---|
| `textDocument/publishDiagnostics` (push) | Stable `E_*`/`W_*` strings in `code`, `source: "cairn"`, span notes as `relatedInformation`, spanless notes folded into `message` as `note:` lines, structured payloads in `data`. Positions are 0-based lines with UTF-16 code-unit columns (the protocol default). |
| `textDocument/diagnostic` (pull) | The same diagnostics, advertised as `diagnosticProvider` with identifier `cairn`, no inter-file dependencies and no workspace diagnostics. A report's `resultId` names the revision of the document's text it was computed on, counted across the session so a reopened document never repeats one; a pull whose `previousResultId` names the stored revision is answered `unchanged` without recomputing. A pull for a document that is not open is refused with `InvalidParams`. |
| `textDocument/completion` | Three closed-vocabulary contexts, detected line-locally so completion works while the document fails to parse: line-opening keywords (top-level items, member commands, `slot` + selector keywords in theme bodies), `mat_slot=` values (union of slot names declared by the document's themes — edition variants union naturally, matching unpinned `cairn check`), and `@` material tokens from the built-in packs, Java ∪ Bedrock. A material token is offered from two vocabularies, in this order: the materials catalogs' abstract tokens, each with the canonical id it resolves to as detail, then every canonical id some version's blocks table declares, the table `E_UNKNOWN_ID` answers from, each with the editions and versions that declare it as detail (`pale_oak_planks`: `java 1.21.4, 26.3; bedrock 1.21.60, 1.26.60`). Only the tokens matching what is typed after the `@` are offered, those starting with it ahead of those with a part after a `_` or `.` that does, and at most `MATERIAL_ITEM_LIMIT` (100) of them; an answer cut there is marked `isIncomplete`, so the client asks again as the token grows. Keyword and slot answers are offered whole. Trigger characters: `@`, `=`, `.`. |
| `textDocument/hover` | Markdown read from a parse of the stored text, so a document that does not parse, and a position past its end, answer `null`. On a top-level item's keyword, a theme selector row's keyword, or a `key=`'s key: its entry in [syntax "Keyword reference"](https://cairn.kage1020.com/spec/syntax/), the text `cairn docs spec/syntax` prints, with the links made absolute; a key quotes the entry its own keyword gives it, then that keyword's, and `id=`, `class=` and `mat_slot=` the entry every member shares. On a `@` token in a theme's `slot` row or a `connect`'s `path=`: what binds it, and per edition the block it names with no version pinned, an abstract token's per-version catalog overrides, and the versions whose blocks tables declare a canonical id when not all do; a selector row's binding says it resolves to no block, since bindings are reserved. Elsewhere on a member's line: the keyword's entry, the member's `id=`, the `intent_state` its line writes, and the `resolved_state` each place it is built in holds per edition, as `cairn lower --edition E` resolves it, each block state with its cell count. A keyword or key the reference does not hold, and every other position, answers `null`. |

## Planned capabilities

| Capability | Spec reference |
|---|---|
| `textDocument/codeAction` — apply the "Suggested fix:" payloads from lint messages | [lint](https://cairn.kage1020.com/spec/lint/), [versioning-editions "Fail-loud and minimum-version inference"](https://cairn.kage1020.com/spec/versioning-editions/) |
| `workspace/executeCommand` — `cairn.info`, `cairn.diffBlocks` | [versioning-editions "The `edition portability` row" and "The `buildable targets` row"](https://cairn.kage1020.com/spec/versioning-editions/), [ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Design notes

- Lint messages are designed to feed the self-correction loop verbatim ([lint](https://cairn.kage1020.com/spec/lint/)). The LSP layer must preserve the "what is wrong / valid candidates / suggested fix" triple intact so a coding agent can act on it without prose paraphrasing.
- Autocomplete is **closed-set first** ([principles P3](https://cairn.kage1020.com/spec/principles/)): the registry table is the source of truth, not a learned vocabulary, so suggestions cannot hallucinate IDs that do not exist in the target `(edition, version)`.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the parser, IR, and lint engine.
- [`cairn-lang-docs`](../../website/rust/README.md) for the keyword reference a hover quotes.
- [`cairn-lang-formats`](../cairn-lang-formats/README.md) for the built-in registry packs backing material-token completion and hover, and the block ids a check with no target weighs.
- [`lsp-server`](https://crates.io/crates/lsp-server) / [`lsp-types`](https://crates.io/crates/lsp-types) for the synchronous stdio transport and protocol types (no async runtime).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
