# cairn-lang-core

Core of the Cairn language: the lexer, the parser, the IR layers above the surface syntax, and the passes that resolve *intent* into a block-array IR.

This crate is the dependency root of every crate above the byte layer; [`cairn-lang-nbt`](../cairn-lang-nbt/README.md), the tree-sitter grammar and [`cairn-lang-docs`](../../website/rust/README.md) stand on their own. It is intentionally backend-free — it knows nothing about NBT byte layout, edition file formats, redstone simulation, the LSP, or WASM bindings. Those live in sibling crates so the compile pipeline stays small and testable.

## Modules

| Module | Role | Spec reference |
|---|---|---|
| `lex`, `parse`, `ast` | Surface syntax to AST, with `MAX_NESTING_DEPTH` / `MAX_EXPR_DEPTH` guards and a dedicated failure diagnoser | [syntax](https://cairn.kage1020.com/spec/syntax/) |
| `logic_library` | The built-in logic library: each primitive a `logic` expression applies, and its notation, which the parser reads | [redstone "Two tiers, and the v1 boundary"](https://cairn.kage1020.com/spec/redstone/) |
| `intent` | Intent IR — struct/site/def bodies reorganised into typed members with roles and meaning-based addresses, with the top-level `edit` lines applied to them, plus the keyword table, semantic levels, the selector grammar a `theme` row and an `edit` share, and the lookup of the member an actuator patch binds | [architecture "The Intent IR is rich and carries invariants"](https://cairn.kage1020.com/spec/architecture/), [syntax "IDs, classes, addresses"](https://cairn.kage1020.com/spec/syntax/), [components-editing-sites "Editing model"](https://cairn.kage1020.com/spec/components-editing-sites/) |
| `check` | The diagnostic-collecting pipeline: arguments, arity, duplicates, edits, nesting, positional form, `@requires`, `@cairn`, `@intended_targets`, materials, member scope, type mismatches | [lint](https://cairn.kage1020.com/spec/lint/) |
| `resolve` | Theme binding, per-edition theme variants, `@requires` parsing, and the three version axes | [materials-themes](https://cairn.kage1020.com/spec/materials-themes/), [versioning-editions "The `edition portability` row" and "The `buildable targets` row"](https://cairn.kage1020.com/spec/versioning-editions/) |
| `block_array` | The universal pivot: walls, openings, roofs, walkways, and material lowering into a palette + index array, and the comparison of a body built with its edits and as written | [architecture "The block-array IR is the universal pivot"](https://cairn.kage1020.com/spec/architecture/) |
| `transliterate` | L1 transliteration: the blocks of an imported structure written back as raw members, folded into `raw_repeat`, with every connection state the compile derives left out, and checked by compiling the result against the stamped target | [ecosystem-interop "Three tiers of transliteration"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `voxel_diff` | The voxel diff of a structure's blocks with a build's: `block_iou`, `state_accuracy` and `residual_ratio`, the cells that differ clustered into regions with their likely causes, and the `edit` lines found, by building with each, to bring the build nearer | [ecosystem-interop "Diffing a lift", "Regions and their causes" and "The suggested patch"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `lock` | Lockfile schema, the hashes that make a build reproducible, and the reading `cairn info` reports of a lock | [versioning-editions "Provenance and lock"](https://cairn.kage1020.com/spec/versioning-editions/) |
| `calver`, `edition`, `ids` | The `@cairn` version value, the edition axis, and the identifier newtypes shared across passes | [versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) |
| `suggest`, `error`, `lines` | "Did you mean …?" lookup over closed vocabularies, spans and positions, line boundaries | [lint](https://cairn.kage1020.com/spec/lint/) |

Everything past the block-array IR — the NBT codec, the format backends, redstone synthesis, the LSP, the WASM bindings — is in the sibling crates listed in [the workspace overview](https://cairn.kage1020.com/development/).

## Diagnostics

`check::Diagnostic` carries a stable `E_*` / `W_*` code, a span, a message, and optional structured `data`. Messages are written in the "what is wrong / valid candidates / suggested fix" shape ([lint](https://cairn.kage1020.com/spec/lint/)) so the same string serves a terminal, an editor, and a self-correcting generation loop without paraphrase. `suggest` backs the "valid candidates" half from the closed vocabulary rather than from a learned one, so a suggestion cannot name an id that does not exist ([principles P3](https://cairn.kage1020.com/spec/principles/)).

## Not yet here

Entity placement and the L2 and L3 tiers of the reverse direction — lifting a transliterated structure into named, themed Cairn — are specified but unimplemented. Roles outside the set `block_array` voxelises (floor, walls, door, window, roof, stair, pressure plate, `level` grouping, and `circuit region` reservations) lower to a `W_DEFERRED_MEMBER` warning rather than to blocks.

## Versioning

`CAIRN_VERSION` is the date-based version of the Cairn release this build belongs to, read from the crate's own package version so it cannot fall behind the workspace. Everything that has to name the compiler that produced an artifact — `cairn --version`, a lockfile's `cairn_version` — reads it from here. It is **not** the Minecraft target version; see [versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) for how the two axes stay separate.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
