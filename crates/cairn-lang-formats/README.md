# cairn-lang-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats, plus the registry packs that say which block ids, and which states of each, a given `(edition, version)` actually has.

Each format is a serializer or deserializer around the **block-array IR**, which the specification names as the universal pivot ([architecture "The block-array IR is the universal pivot"](https://cairn.kage1020.com/spec/architecture/)). Adding a format means adding a backend here — the parser, lint, and theme machinery in [`cairn-lang-core`](../cairn-lang-core/README.md) are untouched.

## Status

Java vanilla `.nbt`, Litematica `.litematic`, Sponge Schematic `.schem` and Bedrock `.mcstructure` writers ship, and so do the built-in registry packs behind `cairn info` and every id check. The Bedrock writer translates each palette entry's canonical blockstate, which is Java's, into the target version's Bedrock block and `states` through the built-in pack's state map ([`bedrock_state`](src/bedrock_state.rs)). A state the map has no form for, or one the Java block does not have, is a hard error (`BedrockStructureError::State`); intent a version can only approximate (stair `shape` before 1.26.60) is dropped with a `W_INTENT_DEGRADED` parity note rather than in silence; and `waterlogged` becomes the structure's water layer.

The `.litematic` writer puts each struct in a file of its own and each site in one file, a region per placement and walkway at its site origin ([ecosystem-interop "Litematica"](https://cairn.kage1020.com/spec/ecosystem-interop/)), and streams each region's packed `BlockStates` from the grid. The `.schem` writer writes version 3, one file per struct, placement and walkway like `.nbt`, each with its site origin as its `Offset` ([ecosystem-interop "Sponge Schematic"](https://cairn.kage1020.com/spec/ecosystem-interop/)), and streams the varint `Blocks.Data` from the grid. Every Java writer takes its block entities from the same encoding and placement as `.nbt`'s, so a block entity the IR carries is written by all three. A reader of each format ships too. The `.nbt`, `.litematic` and `.schem` readers return a `BlockVolume`, the `.litematic` reader one region at a time and the `.schem` reader versions 2 and 3; the `.mcstructure` reader returns a `BedrockVolume`, both of its layers as the file stores them, in the Bedrock vocabulary.

[`import`](src/import.rs) is the library half of `cairn import --mode raw` ([ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/)). It decodes a file into one block grid per structure it holds, a `.litematic` a grid per region, and stamps it with the shipped target the file's saved version maps onto ([ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/)). A Bedrock block is read back into the canonical vocabulary through the target's state map, the inverse of what the writer does. Core's [`transliterate`](../cairn-lang-core/src/transliterate/mod.rs) then writes the grids as raw members and compiles them back against the stamp to check every cell. Each thing the raw members do not carry, block entity data among them, is returned as a `SemanticLoss`, which the CLI prints as `W_SEMANTIC_LOSS`.

The [`info`](src/info.rs) module builds the `cairn info` report, its JSON document and the `note:` lines behind its rows, once for both front ends: the CLI prints it, and the language server's `cairn.info` command returns it.

## Public API

Every item the crate root re-exports, and nothing else. An item reachable only through a module path — `portability::PortabilityEntries`, `registry::AliasCatalog`, anything under `registry::blocks` — is deliberately absent: this table is the names you can write unqualified after `use cairn_lang_formats::*`, not everything that is `pub` somewhere in the tree. `cargo xtask lint` holds it to that in both directions, so a re-export added without a row fails and names the item.

| Item | Role |
|---|---|
| `java_structure::prepare_structure` / `java_structure::JavaStructure` | Check a `BlockArray` against every refusal the Java backend and the NBT encoder have, without building anything; `JavaStructure::write_gzip` then writes the `.nbt`, encoding the per-voxel `blocks` list from the grid as it goes, and can fail only on I/O. What a caller holds to validate every structure before writing any. |
| `java_structure::build_structure_tag` | `BlockArray` → `Compound` (Java vanilla shape). One compound per voxel, so for inspecting a structure rather than writing a large one. |
| `java_structure::write_structure_gzip` | Check and gzip-write in one call, streaming. A refused array leaves the writer untouched. |
| `java_structure::write_compound_gzip` | Gzip-write an already-built root under the empty root name vanilla expects. |
| `java_structure::output_filename` | The stem `cottage` (`BlockArrayIr::artifact_stem` of `struct::cottage`) + `OutputExt::Nbt` → `cottage.nbt`; `OutputExt::Mcstructure` → `cottage.mcstructure`; `OutputExt::Litematic` → `cottage.litematic`; `OutputExt::Schem` → `cottage.schem`. |
| `java_structure::OutputExt` | `Nbt` / `Mcstructure` / `Litematic` / `Schem`. The only way to name an output extension, so no caller ends up with `.nbt.nbt`. |
| `java_structure::JavaStructureError` | `Nbt`, `AbstractPaletteEntry`, `PaletteIndexOutOfRange`, `DimensionOverflow`, `VolumeOverflow`, `ListTooLong`, `VoxelCountMismatch`, `UnencodablePaletteString`. `#[non_exhaustive]`. |
| `java_structure::read_structure` | Gzip-wrapped `.nbt` → `StoredStructure`, reading `blocks` one entry at a time and the fields in whatever order the file stores them. Reads `palette` or the first of `palettes`, and a cell `blocks` lists nothing at, a structure void, as air. |
| `java_structure::StoredStructure` | What it returns: the `DataVersion`, the `BlockVolume`, how many cells were structure voids, how many palettes the file holds, and raw `entities`. |
| `java_structure::StructureReadError` | `Nbt`, `Malformed` (by field path, such as `blocks[3].pos`), `TooLarge`, `PaletteTooLarge`, `PaletteIndexOutOfRange`, `OutsideSize`, `Repeated`. `#[non_exhaustive]`. |
| `java_structure::Compound` | [`cairn-lang-nbt`](../cairn-lang-nbt/README.md)'s tag compound, passed through so building a tag tree needs one dependency rather than two. |
| `litematic::litematic_files` | The `.litematic` files a `BlockArrayIr` writes to: a `LitematicFile` per struct and per site, in the order of each file's first structure. |
| `litematic::LitematicFile` / `litematic::LitematicRegion` | One file's scope (`struct::cottage`, `site::hamlet`), stem and regions; a region's scope, name (the stem its `.nbt` would have), minimum corner and array. |
| `litematic::prepare_litematic` / `litematic::Litematic` | Check a `LitematicFile` for a `JavaTarget`, each region as `prepare_structure` checks it, plus the names and the `i32` metadata; `Litematic::write_gzip` then writes it, packing `BlockStates` from the grid as it goes, and can fail only on I/O. |
| `litematic::LitematicError` | `Region` (a region's `JavaStructureError`), `UnencodableName`, `CornerOverflow`, `MetadataOverflow`. `#[non_exhaustive]`. |
| `litematic::read_litematic` | Gzip-wrapped `.litematic` → `LitematicSchematic`, one region's NBT in memory at a time. Reads `Version` 5 to 7, either corner a region's `Position` and `Size` name, and palettes under `Name` / `Properties` or `id` / `properties`; refuses a region past the block-array volume budget before reading its states. |
| `litematic::LitematicSchematic` / `litematic::LitematicMetadata` / `litematic::ReadRegion` | What it returns: the data version, `Version`, `SubVersion`, the `Metadata` fields as stored, and each region's name, minimum corner, `BlockVolume` and raw `Entities`. |
| `litematic::LitematicReadError` | `Nbt`, `Malformed` (by field path, such as `Regions.home1.Size`), `UnsupportedVersion`, `RegionTooLarge`, `PaletteTooLarge`, `BlockStatesTooShort`, `PaletteIndexOutOfRange`. `#[non_exhaustive]`. |
| `schem::schem_files` / `schem::SchemFile` | The `.schem` files a `BlockArrayIr` writes to, one per structure in the IR's order: each one's scope, stem (the `.nbt` file's), `Offset` (its placement's or walkway's site origin, the origin for a struct) and array. |
| `schem::prepare_schem` / `schem::Schem` | Check a `SchemFile` for a `JavaTarget` as `prepare_structure` checks its array, plus the unsigned-short extents, the palette's spellings and the varint data's length; `Schem::write_gzip` then writes version 3, encoding `Blocks.Data` from the grid as it goes, and can fail only on I/O. |
| `schem::SchemError` | `Structure` (the array's `JavaStructureError`), `DimensionOverflow`, `UnspellableState`, `DuplicateState`, `UnencodableName`, `DataTooLong`. `#[non_exhaustive]`. |
| `schem::read_schem` | Gzip-wrapped `.schem` of version 2 or 3 → `SpongeSchematic`, in whatever order the file stores its fields. Spells a state's id without a namespace as `minecraft:`, maps a sparse palette onto slots in index order, and refuses extents past the block-array volume budget before decoding the data. |
| `schem::SpongeSchematic` | What it returns: `Version`, `DataVersion`, `Metadata` as stored, `Offset`, the `BlockVolume`, and raw `Entities`. |
| `schem::SchemReadError` | `Nbt`, `Malformed` (by field path, such as `Schematic.Blocks.Palette`), `UnsupportedVersion`, `TooLarge`, `PaletteTooLarge`, `DataLength`, `UnknownPaletteIndex`. `#[non_exhaustive]`. |
| `volume::BlockVolume` | Voxels as a file stores them: dims, palette in the file's order, `(y, z, x)` indices, and block entities. What every reader returns, so import has one shape to lift. |
| `volume::StoredBlockEntity` | A block entity as a file stores it: its voxel, its id, and the rest of its NBT. |
| `bedrock_structure::prepare_mcstructure` / `bedrock_structure::McStructure` | Check a `BlockArray` against the Bedrock backend and the NBT encoder and translate its palette through the target version's state map, returning the parity notes alongside; `McStructure::write` then writes the `.mcstructure`, encoding both `block_indices` layers from the grid as it goes, and can fail only on I/O. |
| `bedrock_structure::build_mcstructure_tag` | `BlockArray` → `(Compound, Vec<ParityNote>)` (Bedrock `.mcstructure` shape). Both per-voxel layers as tags, so for inspecting a structure rather than writing a large one. |
| `bedrock_structure::write_mcstructure` | Uncompressed little-endian write of a built root. |
| `bedrock_structure::read_mcstructure` | Uncompressed little-endian `.mcstructure` → `StoredMcStructure`, holding each `block_indices` layer as indices and reordering it from the file's `z`-fastest order into the grid's. Refuses a `format_version` other than 1, and extents past the block-array volume budget before reading a layer. |
| `bedrock_structure::StoredMcStructure` / `bedrock_structure::BedrockVolume` / `bedrock_structure::BedrockBlock` | What it returns: `structure_world_origin`, raw `entities`, and the volume: dims, the palette as the file spells it (each block's Bedrock `name`, typed `states` and block `version`), both layers (`None` for a structure void), and block entities with the position keys stripped from their data. |
| `bedrock_structure::McStructureReadError` | `Nbt`, `Malformed` (by field path, such as `structure.palette.default`), `UnsupportedFormatVersion`, `TooLarge`, `PaletteTooLarge`, `LayerLength`, `PaletteIndexOutOfRange`. `#[non_exhaustive]`. |
| `bedrock_structure::ParityNote` | One intent a palette entry dropped to fit Bedrock: the entry's `id` and `states`, spelled as `portability::PortabilityReport::degraded`'s entries spell them, and the `dropped` intent, worded by the pack's `sensitivity` row for it. The caller surfaces it as `W_INTENT_DEGRADED`; the serializer has no source span, so attributing the note to a structure scope is the caller's job. `#[non_exhaustive]`. |
| `bedrock_structure::BedrockStructureError` | `Nbt`, `AbstractPaletteEntry`, `State`, `PaletteIndexOutOfRange`, `DimensionOverflow`, `VolumeOverflow`, `ListTooLong`, `VoxelCountMismatch`, `UnencodablePaletteString`. `#[non_exhaustive]`. |
| `bedrock_state::translate_states` | One canonical blockstate → its Bedrock block at one version's state map, or the reason it cannot be one. |
| `bedrock_state::StateTranslation` | A successful translation: the Bedrock id, the typed `states` compound, whether the cell holds water, and one entry per piece of intent the version cannot express. Empty `degraded` means lossless. |
| `bedrock_state::BedrockStateError` | `Unmapped`, `UnknownProperty`, `UnknownValue` — each carrying the what-is-wrong / what-is-valid / suggested-fix triple the lint loop reads. |
| `data_version::JavaTarget` / `data_version::resolve_java_target` | `--target <mc_version>` → `(mc_version, DataVersion)`. |
| `data_version::BedrockTarget` / `data_version::resolve_bedrock_target` | `--target <mc_version>` → `(mc_version, order_key, block_version)`: the release's key in Bedrock's version order, and the block-state version its structures carry, which several releases can share. |
| `data_version::UnsupportedTarget` | A `--target` no version table matched: which edition was consulted, the value given, the nearest candidate when one is close enough to suggest, and the supported list. |
| `import::import_raw` | Bytes of one format → `RawImport`: `decode_structure`, then core's `transliterate` at the stamp. What `cairn import --mode raw` calls. |
| `import::decode_structure` / `import::DecodedStructure` | Bytes of one format → the stamped block grids and the losses, before anything is written; `DecodedStructure::registry` is the pack view of the stamped target, and `header` the comment lines the written source opens with. |
| `import::ImportFormat` | `Nbt` / `Litematic` / `Schem` / `Mcstructure`: the format a path's extension names, and the edition it stamps. |
| `import::ImportStamp` / `import::StampFit` | The `(edition, version)` stamped, the version the file was saved at (a `DataVersion` on Java, the newest block version on Bedrock), and how the two stand: `Exact`, `Later` (between two shipped targets) or `Oldest` (before every one). |
| `import::UnshippedVersion` | A file saved past every shipped target of its edition, which no target can check. |
| `import::SemanticLoss` | One thing the raw members do not carry, by the unit it is in when it is in one: the author's intent, block entity data, entities, structure voids, extra palettes, and on Bedrock a block with no canonical state, water that differs from what the written block holds, and a second-layer block other than water. |
| `import::RawImport` | The decoded file and its `Transliteration`. |
| `import::ImportError` | `Structure`, `Litematic`, `Schem`, `McStructure` (each reader's error), `NoRegions`, `Unshipped`, `Transliterate`. `#[non_exhaustive]`. |
| `portability::portability_for_java` | `PortabilityReport` for Java: how many palette entries are portable, degraded, or unsupported, and which. |
| `portability::portability_for_bedrock` | The same for Bedrock, or `InvalidPalette` when the palette carries states the pack should have refused. |
| `portability::PortabilityReport` | The counts and both entry lists, kept together so the figure and the list can never describe different palettes. Fields are private for that reason. |
| `portability::PortabilityCounts` | `portable` / `degraded` / `unsupported`, as `u32` palette-entry counts. |
| `portability::InvalidPalette` | Returned instead of counts when the palette is not one a build could have produced. Carries every refusal, unreworded, because they are one bug's symptoms. |
| `registry::builtin_pack` | The block, alias, absence, material, meaning, connection, and data-version tables for one edition, and Bedrock's state maps, built in and parsed once per process. |
| `registry::load_builtin_pack` | The same pack as a `Result`, for a caller that wants the parse error rather than the panic. |
| `registry::load_from_dir` | The same tables from an external pack directory. Loads it as a Java pack; there is no Bedrock equivalent yet. |
| `registry::RegistryPack` | A loaded and validated pack. Its fields are private and read through accessors, so a pack cannot be built without passing the validators or changed after. |
| `registry::RegistryError` | Everything reading or validating a pack can refuse: a missing or unreadable component, a schema version past what this build understands, a failed validator. |
| `registry::PackSource` | `Builtin` or `Path`. Carried into diagnostics so a reader can tell which directory was read. |
| `registry::PackManifest` | The `pack.json` body: schema version, edition, and the component file references. An unknown edition, such as `"BEDROCK"`, is refused. |
| `registry::PackFiles` | The component references inside a manifest. New components arrive as `Option` fields, so an older pack stays loadable. |

## Registry packs

A pack is the closed vocabulary every id-level answer comes from: which blocks exist in a version and the values each of their states takes, which name a renamed block goes by there, and which canonical token an abstract material resolves to. The Bedrock pack also carries its state maps: `states.json`, how each version writes the canonical states, and the hand-written `sensitivity.json`, what dropping a state no Bedrock state reads costs. Both carry `connections.json`, what each block is to the rules that join panes, fences and walls to their neighbours, the hand-written `absent.json`, the other edition's blocks this one has no counterpart for, each with the reason `E_PARITY_UNSUPPORTED` prints, and the hand-written `meanings.json`, each canonical token whose block split into several and the meaning token whose value picks one. A canonical token a version does not declare is read through those tables before it is refused: the alias group of a block renamed across the edition gives that version's spelling, the meaning table gives the block a meaning token picks, and on Bedrock the state map gives the block it writes a Java block as where no other Java block takes the same forms. `registry-data/<edition>/pack.json` holds the built-in pair, and `load_from_dir` takes an external one. Each table carries a `schema_version` the loader refuses to read past, so a pack written for a newer Cairn fails loudly instead of being half-understood.

The block tables are extracted from the game, and so is the integer each targetable row of a version table writes, Java's `data_version` and Bedrock's `block_version`: Java's from the server jar's `--reports`, Bedrock's from the block palette GeyserMC ships for the release, pinned by commit and SHA-256. Bedrock's state maps come from GeyserMC's Java-to-Bedrock mappings, pinned the same way, read against the `--reports` of the Java release each one follows. Both editions' connection tables are read off the block tags of the server jar of the Java release each version's states follow, the version itself on Java and the release its state map follows on Bedrock, and the block models of that release's client jar. `pack.json` records the file each table came from, under an `extracted` key the compiler does not read. To regenerate an edition's tables, or check the committed ones with `--check`:

```sh
python3 .github/scripts/extract_registry.py java --java <a Java 25 or later>
python3 .github/scripts/extract_registry.py bedrock --java <a Java 25 or later>
```

Because the tables are closed, a suggestion cannot name a block that does not exist in the target — the "valid candidates" half of a diagnostic is looked up, never generated ([principles P3](https://cairn.kage1020.com/spec/principles/)).

## Planned backends

| Format | Edition | Direction | Spec reference |
|---|---|---|---|
| `.nbt` (vanilla structure block) | Java | **write (done)**, **read (done)** | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.mcstructure` | Bedrock | **write (done)**, **read (done)** | [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.litematic` (Litematica) | Java | **write (done)**, **read (done)** | [ecosystem-interop "Litematica"](https://cairn.kage1020.com/spec/ecosystem-interop/), [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/), [ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.schem` (WorldEdit / Sponge) | Java | **write (done)**, **read (done)** | [ecosystem-interop "Sponge Schematic"](https://cairn.kage1020.com/spec/ecosystem-interop/), [ecosystem-interop "Forward direction"](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Forward / reverse contract

- **Forward**: block-array IR → serialize. The compile pipeline writes the IR; each backend encodes it for one format.
- **Reverse**: deserialize → block grids plus a provenance stamp `(edition, version)` → raw members. The compiler performs only a *faithful transliteration* into the raw-centric DSL; semantic lifting is the LLM's job ([ecosystem-interop "Reverse direction: the compiler transliterates, an LLM lifts"](https://cairn.kage1020.com/spec/ecosystem-interop/)). The written source compiles back to the same block array, so the reverse direction shares the forward direction's IR rather than having one of its own ([architecture "The block-array IR is the universal pivot"](https://cairn.kage1020.com/spec/architecture/)). Litematica's regions are not flattened: each is a struct of its own, with its corner kept as a comment ([ecosystem-interop "Import stamping and pitfalls"](https://cairn.kage1020.com/spec/ecosystem-interop/)).

## Out of scope

- Pre-1.13 legacy numeric-id `.schematic`. v1 does not support flattening ([overview "Scope and non-goals"](https://cairn.kage1020.com/spec/overview/), [open-issues "Choices to settle at implementation time"](https://cairn.kage1020.com/spec/open-issues/)).
- SNBT printing — Cairn does not round-trip through SNBT.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the block-array IR types.
- [`cairn-lang-nbt`](../cairn-lang-nbt/README.md) for the byte-level codec.
- [`cairn-lang-redstone`](../cairn-lang-redstone/README.md) for laying each struct's circuit, whose findings the `cairn info` report weighs per edition.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
