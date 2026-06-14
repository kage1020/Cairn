# cairn-core

Core of the Cairn language: the parser, the three-layer IR, and the compiler that resolves *intent*
into a block-array IR.

This crate is the dependency root of every other Cairn crate. It is intentionally backend-free — it
knows nothing about NBT, edition file formats, redstone simulation, the LSP, or WASM bindings. Those
live in sibling crates so that the canonical compile pipeline stays small and testable.

## Status

Skeleton. The implementation is still a stub: it exposes only the [`CAIRN_VERSION`] constant. The
parser, IR types, and resolver pipeline are being filled in chapter by chapter, in lockstep with the
[normative specification](https://cairn.kage1020.com/spec/).

## What lives here

The crate maps directly onto the architecture described in
[`spec/architecture.md`](https://cairn.kage1020.com/spec/architecture/):

| Pipeline stage | Spec reference | Future module |
|---|---|---|
| Lexing + parsing of `.crn` source | [syntax](https://cairn.kage1020.com/spec/syntax/) | `parser` |
| Intent IR (named members, classes, `mat_slot`, `intent_state`) | [architecture §3.2](https://cairn.kage1020.com/spec/architecture/), [blockstate](https://cairn.kage1020.com/spec/blockstate/) | `ir::intent` |
| Semantic / Component-Theme IR (themes, `def`, `site`) | [materials-themes](https://cairn.kage1020.com/spec/materials-themes/), [components-editing-sites](https://cairn.kage1020.com/spec/components-editing-sites/) | `ir::semantic` |
| Phase-ordered evaluation | [compilation](https://cairn.kage1020.com/spec/compilation/) | `resolve` |
| Block-array IR (the universal pivot) | [architecture §3.1](https://cairn.kage1020.com/spec/architecture/) | `ir::block_array` |
| Lint / constraint validation | [lint](https://cairn.kage1020.com/spec/lint/), [versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) | `lint` |
| Editing & patch DSL | [components-editing-sites §9.2](https://cairn.kage1020.com/spec/components-editing-sites/) | `edit` |
| Provenance + lockfile | [versioning-editions §10.6](https://cairn.kage1020.com/spec/versioning-editions/) | `provenance`, `lock` |

Everything beyond the block-array IR — NBT codec, schematic format backends, redstone synthesis,
LSP, WASM — is implemented in the sibling crates listed in [the workspace
overview](https://cairn.kage1020.com/development/).

## Versioning

`cairn-core` exposes [`CAIRN_VERSION`] (`"2026.06"`), the date-based version of the Cairn release
this build belongs to. This is **not** the Minecraft target version; see
[versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) for how the two axes are kept separate.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
