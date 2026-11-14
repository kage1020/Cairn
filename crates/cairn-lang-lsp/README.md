# cairn-lang-lsp

Language Server Protocol implementation for Cairn editors. It puts the parser and lint diagnostics from [`cairn-lang-core`](../cairn-lang-core/README.md) in the margin as you type, completes material tokens and describes what the cursor rests on, so the write-check-fix loop described in [lint](https://cairn.kage1020.com/spec/lint/) happens in the editor rather than in a terminal. The [VS Code extension](../../editors/vscode/README.md) is a thin client over this binary.

## Status

The `cairn-lsp` binary speaks standard LSP over stdio: it negotiates full-content document sync at `initialize`, reports diagnostics against what the client's [settings](#settings) pin — with nothing pinned, what a `cairn check` with no `--edition` reports: the same `parse → lower → check` passes and the lowering-stage findings every target that reads their line raises, without the `@intended_targets` findings the CLI merges in — and answers `textDocument/completion` from the closed vocabularies. A client whose `initialize` declares `textDocument.diagnostic` pulls them with `textDocument/diagnostic` and is never published to; any other client is pushed `textDocument/publishDiagnostics` on every `didOpen`/`didChange`, cleared on `didClose`. Diagnostics are computed on a second thread, so a completion request is not kept waiting behind them; that thread answers each pull ahead of the publications still queued, answers a pull the client cancels before the thread reaches it with `RequestCancelled`, and skips a pushed revision a newer one has replaced. If that thread dies, by a panic or a failed write, the server exits with an error at once in either mode, rather than leave a pull unanswered. It answers `textDocument/hover` from the stored text, and `textDocument/codeAction` with quick-fixes read from each diagnostic's payload.

## Shipped capabilities

| Capability | Notes |
|---|---|
| `textDocument/publishDiagnostics` (push, to a client that does not pull) | Stable `E_*`/`W_*` strings in `code`, `source: "cairn"`, span notes as `relatedInformation`, spanless notes folded into `message` as `note:` lines, structured payloads in `data`. Positions are 0-based lines with UTF-16 code-unit columns (the protocol default). |
| `textDocument/diagnostic` (pull) | The same diagnostics, advertised as `diagnosticProvider` with identifier `cairn`, no inter-file dependencies and no workspace diagnostics. A report's `resultId` names the revision of the document's text it was computed on, counted across the session so a reopened document never repeats one; a pull whose `previousResultId` names the stored revision is answered `unchanged` without recomputing. A pull for a document that is not open is answered `ServerCancelled` with `retriggerRequest: false`, since the client pulls again once it opens one; a pull whose params do not parse is refused with `InvalidParams`. |
| `textDocument/completion` | Three closed-vocabulary contexts, detected line-locally so completion works while the document fails to parse: line-opening keywords (top-level items, member commands, `slot` + selector keywords in theme bodies), `mat_slot=` values (union of slot names declared by the document's themes — edition variants union naturally, matching unpinned `cairn check`), and `@` material tokens from the built-in packs, Java ∪ Bedrock. A material token is offered from two vocabularies: the materials catalogs' abstract tokens, each with what it resolves to in each edition as detail, the default id and then any version that spells it otherwise (`floor.stone.smooth`: `java minecraft:stone_bricks; bedrock minecraft:stone_bricks (1.21.0: minecraft:stonebrick)`), and every canonical id some version's blocks table declares, each with the editions and versions that declare it as detail (`pale_oak_planks`: `java 1.21.4, 26.3; bedrock 1.21.60, 1.26.60`). `E_UNKNOWN_ID` checks a pinned target against its own version's table, so an id offered here can still be unknown there. A token is matched, ignoring case, on what is typed between the `@` and the cursor, while the edit replaces the whole token: a label matches when it starts with that text, when it does from just after some `_` or `.`, or when it holds its characters in order. The server answers abstract tokens before canonical ids and, within each vocabulary, those three kinds of match in that order, so `@cobble` is answered `floor.stone.cobble`, an inner match, before `cobblestone`, a leading one. That order decides which items survive the cut at `MATERIAL_ITEM_LIMIT` (100); an answer cut there is marked `isIncomplete`, so the client asks again as the token grows. A client that sorts by its own match score, as VS Code does, keeps the server's order only among items that score alike. Keyword and slot answers are offered whole. Trigger characters: `@`, `=`, `.`. |
| `textDocument/hover` | Markdown read from a parse of the stored text, so a document that does not parse, and a position past its end, answer `null`. On a top-level item's keyword, a theme selector row's keyword, or a `key=`'s key: its entry in [syntax "Keyword reference"](https://cairn.kage1020.com/spec/syntax/), the text `cairn docs spec/syntax` prints, with the links made absolute; a key quotes the entry its own keyword gives it, then that keyword's, and `id=`, `class=` and `mat_slot=` the entry every member shares. On a `@` token in a theme's `slot` row or a `connect`'s `path=`: what binds it, and per edition the block it names with no version pinned, an abstract token's per-version catalog overrides, and the versions whose blocks tables declare a canonical id when not all do; a selector row's binding says it resolves to no block, since bindings are reserved. Elsewhere on a member's line: the keyword's entry, the member's `id=`, the `intent_state` its line writes, and the `resolved_state` each place it is built in holds per edition, as `cairn lower --edition E` resolves it, each block state with its cell count. A keyword or key the reference does not hold, and every other position, answers `null`. |
| `textDocument/codeAction` | Quick-fixes (kind `quickfix`) for the diagnostics the request carries back, each built from the diagnostic's `data` and never from its message ([lint "Machine-readable payload"](https://cairn.kage1020.com/spec/lint/#112-machine-readable-payload)): for an `authored` `E_UNKNOWN_ID`, each of its `aliases`, the first `isPreferred`, then its `suggestion`; for `E_UNKNOWN_KEYWORD` and `E_UNKNOWN_ARGUMENT`, the `suggestion`; for `E_VERSION_CAP`, an edit lowering the floor to the target, and, for a client that runs [`cairn.setTarget`](#client-commands), that command with the lowest target the floors admit. A diagnostic whose range no longer holds the text its payload names gets no fix, and one whose `source` is not `cairn` is ignored. A request whose `only` lists neither `quickfix` nor the empty kind is answered `[]`, and one for a document that is not open is refused with `InvalidParams`. |
| `workspace/didChangeConfiguration` | Reads the pin from `settings.cairn`; see [Settings](#settings). |

## Settings

The server reads `cairn.edition` (`java` or `bedrock`) and `cairn.target` (a version the edition ships, or `latest`) from the `settings` of each `workspace/didChangeConfiguration`; it does not ask for them with `workspace/configuration`. They pin what the diagnostics are computed against ([lint "Materials and targets"](https://cairn.kage1020.com/spec/lint/#materials-and-targets)):

| Settings | Diagnostics |
|---|---|
| Neither, or both empty | What `cairn check` reports. |
| `cairn.edition` alone | What `cairn check --edition E` reports: that edition's theme variants and `@edition` guards. |
| Both | What `cairn compile --edition E --target V` refuses on before it writes anything: the ids, states and parity the target's tables check, and the target held to the floors, as `E_VERSION_CAP` or `E_REQUIRES_UNORDERABLE`. A target a floor refuses pins no version. `E_PARTIAL_BUILD`, a verdict on the build, is not reported. |

A setting `cairn compile` would refuse — a target with no edition, a name that is no edition, a target the edition does not ship, a value that is not a string — is shown as a `window/showMessage` error, and the server pins the edition alone, or nothing when the edition is the setting refused. A changed pin re-reports every open document: a pushed client is published to again, with no `version` since no text changed; a pulling client is renumbered, so its next pull is answered in full, and is sent `workspace/diagnostic/refresh` when its `initialize` declares `workspace.diagnostics.refreshSupport`.

## Client commands

| Command | Arguments | What the client does |
|---|---|---|
| `cairn.setTarget` | The version, as a string | Sets `cairn.target` to it, which pins it once the change reaches the server. |

A client says which it runs in `initialize`, under `capabilities.experimental.commands.commands`, as a list of command names. The server offers a command only to a client that lists it.

## Planned capabilities

| Capability | Spec reference |
|---|---|
| `workspace/executeCommand` — `cairn.info`, `cairn.diffBlocks` | [versioning-editions "The `edition portability` row" and "The `buildable targets` row"](https://cairn.kage1020.com/spec/versioning-editions/), [ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Design notes

- Lint messages are designed to feed the self-correction loop verbatim ([lint](https://cairn.kage1020.com/spec/lint/)). The LSP layer must preserve the "what is wrong / valid candidates / suggested fix" triple intact so a coding agent can act on it without prose paraphrasing.
- Autocomplete is **closed-set first** ([principles P3](https://cairn.kage1020.com/spec/principles/)): the registry table is the source of truth, not a learned vocabulary, so suggestions cannot hallucinate IDs that do not exist in the target `(edition, version)`.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the parser, IR, and lint engine.
- [`cairn-lang-docs`](../../website/rust/README.md) for the keyword reference a hover quotes.
- [`cairn-lang-formats`](../cairn-lang-formats/README.md) for the built-in registry packs backing material-token completion and hover, the block ids a check with no target weighs, and the tables and floors a pinned target is checked against.
- [`lsp-server`](https://crates.io/crates/lsp-server) / [`lsp-types`](https://crates.io/crates/lsp-types) for the synchronous stdio transport and protocol types (no async runtime), and [`crossbeam-channel`](https://crates.io/crates/crossbeam-channel), whose channels `lsp-server` hands over, to wait on the client and the diagnostics thread at once.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
