# cairn-lang-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats, plus the registry packs that say which block ids, and which states of each, a given `(edition, version)` actually has.

Each format is a serializer or deserializer around the **block-array IR**, which the specification names as the universal pivot ([architecture "The block-array IR is the universal pivot"](https://cairn.kage1020.com/spec/architecture/)). Adding a format means adding a backend here — the parser, lint, and theme machinery in [`cairn-lang-core`](../cairn-lang-core/README.md) are untouched.

## Status

Java vanilla `.nbt` and Bedrock `.mcstructure` writers ship, and so do the built-in registry packs behind `cairn info` and every id check. The Bedrock writer translates each palette entry's canonical blockstate, which is Java's, into the target version's Bedrock block and `states` through the built-in pack's state map ([`bedrock_state`](src/bedrock_state.rs)). A state the map has no form for, or one the Java block does not have, is a hard error (`BedrockStructureError::State`); intent a version can only approximate (stair `shape` before 1.26.60) is dropped with a `W_INTENT_DEGRADED` parity note rather than in silence; and `waterlogged` becomes the structure's water layer.

Litematica `.litematic` and WorldEdit `.schem` are still to land, and no reverse-direction (file → IR) backend ships yet.

## Public API

Every item the crate root re-exports, and nothing else. An item reachable only through a module path — `portability::PortabilityEntries`, `registry::AliasCatalog`, anything under `registry::blocks` — is deliberately absent: this table is the names you can write unqualified after `use cairn_lang_formats::*`, not everything that is `pub` somewhere in the tree. `cargo xtask lint` holds it to that in both directions, so a re-export added without a row fails and names the item.

| Item | Role |
|---|---|
| `java_structure::prepare_structure` / `java_structure::JavaStructure` | Check a `BlockArray` against every refusal the Java backend and the NBT encoder have, without building anything; `JavaStructure::write_gzip` then writes the `.nbt`, encoding the per-voxel `blocks` list from the grid as it goes, and can fail only on I/O. What a caller holds to validate every structure before writing any. |
| `java_structure::build_structure_tag` | `BlockArray` → `Compound` (Java vanilla shape). One compound per voxel, so for inspecting a structure rather than writing a large one. |
| `java_structure::write_structure_gzip` | Check and gzip-write in one call, streaming. A refused array leaves the writer untouched. |
| `java_structure::write_compound_gzip` | Gzip-write an already-built root under the empty root name vanilla expects. |
| `java_structure::output_filename` | The stem `cottage` (`BlockArrayIr::artifact_stem` of `struct::cottage`) + `OutputExt::Nbt` → `cottage.nbt`; `OutputExt::Mcstructure` → `cottage.mcstructure`. |
| `java_structure::OutputExt` | `Nbt` / `Mcstructure`. The only way to name an output extension, so no caller ends up with `.nbt.nbt`. |
| `java_structure::JavaStructureError` | `Nbt`, `AbstractPaletteEntry`, `PaletteIndexOutOfRange`, `DimensionOverflow`, `VolumeOverflow`, `ListTooLong`, `VoxelCountMismatch`, `UnencodablePaletteString`. `#[non_exhaustive]`. |
| `java_structure::Compound` | [`cairn-lang-nbt`](../cairn-lang-nbt/README.md)'s tag compound, passed through so building a tag tree needs one dependency rather than two. |
| `bedrock_structure::prepare_mcstructure` / `bedrock_structure::McStructure` | Check a `BlockArray` against the Bedrock backend and the NBT encoder and translate its palette through the target version's state map, returning the parity notes alongside; `McStructure::write` then writes the `.mcstructure`, encoding both `block_indices` layers from the grid as it goes, and can fail only on I/O. |
| `bedrock_structure::build_mcstructure_tag` | `BlockArray` → `(Compound, Vec<ParityNote>)` (Bedrock `.mcstructure` shape). Both per-voxel layers as tags, so for inspecting a structure rather than writing a large one. |
| `bedrock_structure::write_mcstructure` | Uncompressed little-endian write of a built root. |
| `bedrock_structure::ParityNote` | One intent a palette entry dropped to fit Bedrock: the entry's `id` and `states`, spelled as `portability::PortabilityReport::degraded`'s entries spell them, and the `dropped` intent, worded by the pack's `sensitivity` row for it. The caller surfaces it as `W_INTENT_DEGRADED`; the serializer has no source span, so attributing the note to a structure scope is the caller's job. `#[non_exhaustive]`. |
| `bedrock_structure::BedrockStructureError` | `Nbt`, `AbstractPaletteEntry`, `State`, `PaletteIndexOutOfRange`, `DimensionOverflow`, `VolumeOverflow`, `ListTooLong`, `VoxelCountMismatch`, `UnencodablePaletteString`. `#[non_exhaustive]`. |
| `bedrock_state::translate_states` | One canonical blockstate → its Bedrock block at one version's state map, or the reason it cannot be one. |
| `bedrock_state::StateTranslation` | A successful translation: the Bedrock id, the typed `states` compound, whether the cell holds water, and one entry per piece of intent the version cannot express. Empty `degraded` means lossless. |
| `bedrock_state::BedrockStateError` | `Unmapped`, `UnknownProperty`, `UnknownValue` — each carrying the what-is-wrong / what-is-valid / suggested-fix triple the lint loop reads. |
| `data_version::JavaTarget` / `data_version::resolve_java_target` | `--target <mc_version>` → `(mc_version, DataVersion)`. |
| `data_version::BedrockTarget` / `data_version::resolve_bedrock_target` | `--target <mc_version>` → `(mc_version, order_key, block_version)`: the release's key in Bedrock's version order, and the block-state version its structures carry, which several releases can share. |
| `data_version::UnsupportedTarget` | A `--target` no version table matched: which edition was consulted, the value given, the nearest candidate when one is close enough to suggest, and the supported list. |
| `portability::portability_for_java` | `PortabilityReport` for Java: how many palette entries are portable, degraded, or unsupported, and which. |
| `portability::portability_for_bedrock` | The same for Bedrock, or `InvalidPalette` when the palette carries states the pack should have refused. |
| `portability::PortabilityReport` | The counts and both entry lists, kept together so the figure and the list can never describe different palettes. Fields are private for that reason. |
| `portability::PortabilityCounts` | `portable` / `degraded` / `unsupported`, as `u32` palette-entry counts. |
| `portability::InvalidPalette` | Returned instead of counts when the palette is not one a build could have produced. Carries every refusal, unreworded, because they are one bug's symptoms. |
| `registry::builtin_pack` | The block, alias, material, connection, and data-version tables for one edition, and Bedrock's state maps, built in and parsed once per process. |
| `registry::load_builtin_pack` | The same pack as a `Result`, for a caller that wants the parse error rather than the panic. |
| `registry::load_from_dir` | The same tables from an external pack directory. Loads it as a Java pack; there is no Bedrock equivalent yet. |
| `registry::RegistryPack` | A loaded and validated pack. Its fields are private and read through accessors, so a pack cannot be built without passing the validators or changed after. |
| `registry::RegistryError` | Everything reading or validating a pack can refuse: a missing or unreadable component, a schema version past what this build understands, a failed validator. |
| `registry::PackSource` | `Builtin` or `Path`. Carried into diagnostics so a reader can tell which directory was read. |
| `registry::PackManifest` | The `pack.json` body: schema version, edition, and the component file references. An unknown edition, such as `"BEDROCK"`, is refused. |
| `registry::PackFiles` | The component references inside a manifest. New components arrive as `Option` fields, so an older pack stays loadable. |

## Registry packs

A pack is the closed vocabulary every id-level answer comes from: which blocks exist in a version and the values each of their states takes, which name a renamed block goes by there, and which canonical token an abstract material resolves to. The Bedrock pack also carries its state maps: `states.json`, how each version writes the canonical states, and the hand-written `sensitivity.json`, what dropping a state no Bedrock state reads costs. Both carry `connections.json`, what each block is to the rules that join panes, fences and walls to their neighbours. `registry-data/<edition>/pack.json` holds the built-in pair, and `load_from_dir` takes an external one. Each table carries a `schema_version` the loader refuses to read past, so a pack written for a newer Cairn fails loudly instead of being half-understood.

The block tables are extracted from the game, and so is the integer each targetable row of a version table writes, Java's `data_version` and Bedrock's `block_version`: Java's from the server jar's `--reports`, Bedrock's from the block palette GeyserMC ships for the release, pinned by commit and SHA-256. Bedrock's state maps come from GeyserMC's Java-to-Bedrock mappings, pinned the same way, read against the `--reports` of the Java release each one follows. Both editions' connection tables are read off the block tags of that Java release's server jar and the block models of its client jar. `pack.json` records the file each table came from, under an `extracted` key the compiler does not read. To regenerate an edition's tables, or check the committed ones with `--check`:

```sh
python3 .github/scripts/extract_registry.py java --java <a Java 25 or later>
python3 .github/scripts/extract_registry.py bedrock --java <a Java 25 or later>
```

Because the tables are closed, a suggestion cannot name a block that does not exist in the target — the "valid candidates" half of a diagnostic is looked up, never generated ([principles P3](https://cairn.kage1020.com/spec/principles/)).

## Planned backends

| Format | Edition | Direction | Spec reference |
|---|---|---|---|
| `.nbt` (vanilla structure block) | Java | **write (done)**, read | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.mcstructure` | Bedrock | **write (done)**, read | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.litematic` (Litematica) | Java | read / write | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/), [ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.schem` (WorldEdit / Sponge) | Java | read / write | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Forward / reverse contract

- **Forward**: block-array IR → serialize. The compile pipeline writes the IR; each backend encodes it for one format.
- **Reverse**: deserialize → block-array IR plus a provenance stamp `(edition, version)`. The compiler performs only a *faithful transliteration* into the raw-centric DSL; semantic lifting is the LLM's job ([ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/)). Litematica's multi-region structure is preserved as `site` placement, not flattened ([ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/)).

## Out of scope

- Pre-1.13 legacy numeric-id `.schematic`. v1 does not support flattening ([overview "Scope and non-goals"](https://cairn.kage1020.com/spec/overview/), [open-issues "Choices to settle at implementation time"](https://cairn.kage1020.com/spec/open-issues/)).
- SNBT printing — Cairn does not round-trip through SNBT.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the block-array IR types.
- [`cairn-lang-nbt`](../cairn-lang-nbt/README.md) for the byte-level codec.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
