# cairn-lang-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats, plus the registry packs that say which block ids a given `(edition, version)` actually has.

Each format is a serializer or deserializer around the **block-array IR**, which the specification names as the universal pivot ([architecture §3.1](https://cairn.kage1020.com/spec/architecture/)). Adding a format means adding a backend here — the parser, lint, and theme machinery in [`cairn-lang-core`](../cairn-lang-core/README.md) are untouched.

## Status

Java vanilla `.nbt` and Bedrock `.mcstructure` writers ship, and so do the built-in registry packs behind `cairn info` and every id check. The Bedrock writer translates blockstate properties into Bedrock's `states` vocabulary for the families [`bedrock_state`](src/bedrock_state.rs) covers; a property outside that vocabulary is a hard error (`BedrockStructureError::State`), and intent Bedrock can only approximate (stair `shape`) is dropped with a `W_INTENT_DEGRADED` parity note rather than in silence.

Litematica `.litematic` and WorldEdit `.schem` are still to land, and no reverse-direction (file → IR) backend ships yet.

## Public API

| Item | Role |
|---|---|
| `java_structure::build_structure_tag` | `BlockArray` → `Compound` (Java vanilla shape). |
| `java_structure::write_structure_gzip` | Build and gzip-write in one call. |
| `java_structure::output_filename` | `struct::cottage` + `OutputExt::Nbt` → `cottage.nbt`; `OutputExt::Mcstructure` → `cottage.mcstructure`. |
| `java_structure::JavaStructureError` | `Nbt`, `AbstractPaletteEntry`, `PaletteIndexOutOfRange`, `DimensionOverflow`. |
| `bedrock_structure::build_mcstructure_tag` | `BlockArray` → `(Compound, Vec<ParityNote>)` (Bedrock `.mcstructure` shape). |
| `bedrock_structure::write_mcstructure` | Uncompressed little-endian write of a built root. |
| `bedrock_structure::BedrockStructureError` | `Nbt`, `AbstractPaletteEntry`, `State`, `PaletteIndexOutOfRange`, `DimensionOverflow`. |
| `bedrock_state::translate_states` | One Java blockstate → the Bedrock `states` compound, or the reason it cannot be one. |
| `data_version::JavaTarget` / `resolve_java_target` | `--target <mc_version>` → `(mc_version, DataVersion)`. |
| `data_version::BedrockTarget` / `resolve_bedrock_target` | `--target <mc_version>` → `(mc_version, block_version)`. |
| `portability::portability_for_java` / `_for_bedrock` | Per-edition `PortabilityReport`: how many palette entries are portable, degraded, or unsupported, and which. |
| `registry::builtin_java` / `builtin_bedrock` / `load_from_dir` | The block, alias, material, and data-version tables for one edition, built in or loaded from a pack directory. |

## Registry packs

A pack is the closed vocabulary every id-level answer comes from: which blocks exist in a version, which name a renamed block goes by there, and which canonical token an abstract material resolves to. `registry-data/<edition>/pack.json` holds the built-in pair, and `load_from_dir` takes an external one. Each table carries a `schema_version` the loader refuses to read past, so a pack written for a newer Cairn fails loudly instead of being half-understood.

Because the tables are closed, a suggestion cannot name a block that does not exist in the target — the "valid candidates" half of a diagnostic is looked up, never generated ([principles P3](https://cairn.kage1020.com/spec/principles/)).

## Planned backends

| Format | Edition | Direction | Spec reference |
|---|---|---|---|
| `.nbt` (vanilla structure block) | Java | **write (done)**, read | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.mcstructure` | Bedrock | **write (done)**, read | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.litematic` (Litematica) | Java | read / write | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/), [§12.4](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.schem` (WorldEdit / Sponge) | Java | read / write | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Forward / reverse contract

- **Forward**: block-array IR → serialize. The compile pipeline writes the IR; each backend encodes it for one format.
- **Reverse**: deserialize → block-array IR plus a provenance stamp `(edition, version)`. The compiler performs only a *faithful transliteration* into the raw-centric DSL; semantic lifting is the LLM's job ([ecosystem-interop §12.2](https://cairn.kage1020.com/spec/ecosystem-interop/)). Litematica's multi-region structure is preserved as `site` placement, not flattened ([§12.4](https://cairn.kage1020.com/spec/ecosystem-interop/)).

## Out of scope

- Pre-1.13 legacy numeric-id `.schematic`. v1 does not support flattening ([overview §1.3](https://cairn.kage1020.com/spec/overview/), [open-issues §15.1](https://cairn.kage1020.com/spec/open-issues/)).
- SNBT printing — Cairn does not round-trip through SNBT.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the block-array IR types.
- [`cairn-lang-nbt`](../cairn-lang-nbt/README.md) for the byte-level codec.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
