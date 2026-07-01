# cairn-lang-lsp

Language Server Protocol implementation for Cairn editors. Surfaces parser and lint diagnostics from
[`cairn-lang-core`](../cairn-lang-core/README.md), autocompletes canonical material tokens, and exposes the
self-correction loop described in [lint](https://cairn.kage1020.com/spec/lint/) and
[evaluation](https://cairn.kage1020.com/spec/evaluation/) in a form an editor (or an LLM acting through an editor) can
consume incrementally.

## Status

Skeleton. The server binary, transports, and capability negotiation are not yet wired.

## Planned capabilities

| Capability | Spec reference |
|---|---|
| `textDocument/diagnostic` — syntax, geometry, attachment, support, fluid, version_caps, edit_stability, redstone | [lint](https://cairn.kage1020.com/spec/lint/) |
| `textDocument/completion` — canonical material tokens, `mat_slot` names, theme selectors | [materials-themes §7.2](https://cairn.kage1020.com/spec/materials-themes/) |
| `textDocument/hover` — block primitive docs, blockstate intent vs resolved view | [blockstate §6.2](https://cairn.kage1020.com/spec/blockstate/) |
| `textDocument/codeAction` — apply the "Suggested fix:" payloads from lint messages | [lint](https://cairn.kage1020.com/spec/lint/), [versioning-editions §10.4](https://cairn.kage1020.com/spec/versioning-editions/) |
| `workspace/executeCommand` — `cairn.info`, `cairn.diffBlocks` | [versioning-editions §10.5](https://cairn.kage1020.com/spec/versioning-editions/), [ecosystem-interop §12.2](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Design notes

- Lint messages are designed to feed the self-correction loop verbatim
  ([lint §11](https://cairn.kage1020.com/spec/lint/)). The LSP layer must preserve the "what is wrong / valid
  candidates / suggested fix" triple intact so a coding agent can act on it without prose
  paraphrasing.
- Autocomplete is **closed-set first** ([principles P3](https://cairn.kage1020.com/spec/principles/)): the registry
  table is the source of truth, not a learned vocabulary, so suggestions cannot hallucinate IDs
  that do not exist in the target `(edition, version)`.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the parser, IR, and lint engine.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
