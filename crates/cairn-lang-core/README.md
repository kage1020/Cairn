# cairn-lang-core

Core of the Cairn language: the lexer, the parser, the IR layers above the surface syntax, and the passes that resolve *intent* into a block-array IR.

This crate is the dependency root of every crate above the byte layer; [`cairn-lang-nbt`](../cairn-lang-nbt/README.md) and the tree-sitter grammar stand on their own. It is intentionally backend-free — it knows nothing about NBT byte layout, edition file formats, redstone simulation, the LSP, or WASM bindings. Those live in sibling crates so the compile pipeline stays small and testable.

## Modules

| Module | Role | Spec reference |
|---|---|---|
| `lex`, `parse`, `ast` | Surface syntax to AST, with `MAX_NESTING_DEPTH` / `MAX_EXPR_DEPTH` guards and a dedicated failure diagnoser | [syntax](https://cairn.kage1020.com/spec/syntax/) |
| `intent` | Intent IR — struct/site/def bodies reorganised into typed members with roles, plus the keyword table and semantic levels | [architecture §3.2](https://cairn.kage1020.com/spec/architecture/) |
| `check` | The diagnostic-collecting pipeline: arguments, arity, duplicates, nesting, positional form, `@requires`, `@cairn`, `@intended_targets`, materials, member scope, type mismatches | [lint](https://cairn.kage1020.com/spec/lint/) |
| `resolve` | Theme binding, per-edition theme variants, `@requires` parsing, and the three version axes | [materials-themes](https://cairn.kage1020.com/spec/materials-themes/), [versioning-editions §10.5](https://cairn.kage1020.com/spec/versioning-editions/) |
| `block_array` | The universal pivot: walls, openings, roofs, walkways, and material lowering into a palette + index array | [architecture §3.1](https://cairn.kage1020.com/spec/architecture/) |
| `lock` | Lockfile schema and the hashes that make a build reproducible | [versioning-editions §10.6](https://cairn.kage1020.com/spec/versioning-editions/) |
| `calver`, `edition`, `ids` | The `@cairn` version value, the edition axis, and the identifier newtypes shared across passes | [versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) |
| `suggest`, `error`, `lines` | "Did you mean …?" lookup over closed vocabularies, spans and positions, line boundaries | [lint §11](https://cairn.kage1020.com/spec/lint/) |

Everything past the block-array IR — the NBT codec, the format backends, redstone synthesis, the LSP, the WASM bindings — is in the sibling crates listed in [the workspace overview](https://cairn.kage1020.com/development/).

## Diagnostics

`check::Diagnostic` carries a stable `E_*` / `W_*` code, a span, a message, and optional structured `data`. Messages are written in the "what is wrong / valid candidates / suggested fix" shape ([lint §11](https://cairn.kage1020.com/spec/lint/)) so the same string serves a terminal, an editor, and a self-correcting generation loop without paraphrase. `suggest` backs the "valid candidates" half from the closed vocabulary rather than from a learned one, so a suggestion cannot name an id that does not exist ([principles P3](https://cairn.kage1020.com/spec/principles/)).

## Not yet here

Entity placement, the editing and patch DSL ([components-editing-sites §9.2](https://cairn.kage1020.com/spec/components-editing-sites/)), and the reverse direction — lifting an imported structure back into idiomatic Cairn — are specified but unimplemented. Roles outside the set `block_array` voxelises today (floor, walls, door, window, roof, stair, pressure plate, `level` grouping, and `circuit region` reservations) lower to a `W_DEFERRED_MEMBER` warning rather than to blocks.

## Versioning

`CAIRN_VERSION` is the date-based version of the Cairn release this build belongs to, read from the crate's own package version so it cannot fall behind the workspace. Everything that has to name the compiler that produced an artifact — `cairn --version`, a lockfile's `cairn_version` — reads it from here. It is **not** the Minecraft target version; see [versioning-editions](https://cairn.kage1020.com/spec/versioning-editions/) for how the two axes stay separate.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
