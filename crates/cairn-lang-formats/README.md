# cairn-lang-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats, plus the registry packs that say which block ids a given `(edition, version)` actually has.

Each format is a serializer or deserializer around the **block-array IR**, which the specification names as the universal pivot ([architecture "The block-array IR is the universal pivot"](https://cairn.kage1020.com/spec/architecture/)). Adding a format means adding a backend here — the parser, lint, and theme machinery in [`cairn-lang-core`](../cairn-lang-core/README.md) are untouched.

## Status

Java vanilla `.nbt` and Bedrock `.mcstructure` writers ship, and so do the built-in registry packs behind `cairn info` and every id check. The Bedrock writer translates blockstate properties into Bedrock's `states` vocabulary for the families [`bedrock_state`](src/bedrock_state.rs) covers; a property outside that vocabulary is a hard error (`BedrockStructureError::State`), and intent Bedrock can only approximate (stair `shape`) is dropped with a `W_INTENT_DEGRADED` parity note rather than in silence.

Litematica `.litematic` and WorldEdit `.schem` are still to land, and no reverse-direction (file → IR) backend ships yet.

## Public API

Every item the crate root re-exports, and nothing else. An item reachable only through a module path — `portability::PortabilityEntries`, `registry::AliasCatalog`, anything under `registry::manifest` — is deliberately absent: this table answers "what can I call after `use cairn_lang_formats::*`", not "what is `pub` somewhere in the tree". A test holds it to that in both directions, so a re-export added without a row fails the build.

| Item | Role |
|---|---|
| `java_structure::build_structure_tag` | `BlockArray` → `Compound` (Java vanilla shape). |
| `java_structure::write_structure_gzip` | Build and gzip-write in one call. |
| `java_structure::write_compound_gzip` | Gzip-write an already-built root under the empty root name vanilla expects. Split out so a caller can build every tree before touching the filesystem. |
| `java_structure::output_filename` | `struct::cottage` + `OutputExt::Nbt` → `cottage.nbt`; `OutputExt::Mcstructure` → `cottage.mcstructure`. |
| `java_structure::OutputExt` | `Nbt` / `Mcstructure`. The only way to name an output extension, so no caller ends up with `.nbt.nbt`. |
| `java_structure::JavaStructureError` | `Nbt`, `AbstractPaletteEntry`, `PaletteIndexOutOfRange`, `DimensionOverflow`. |
| `java_structure::Compound` | [`cairn-lang-nbt`](../cairn-lang-nbt/README.md)'s tag compound, passed through so building a tag tree needs one dependency rather than two. |
| `bedrock_structure::build_mcstructure_tag` | `BlockArray` → `(Compound, Vec<ParityNote>)` (Bedrock `.mcstructure` shape). |
| `bedrock_structure::write_mcstructure` | Uncompressed little-endian write of a built root. |
| `bedrock_structure::ParityNote` | One degraded palette entry: its concrete id and the sentence the caller surfaces as `W_INTENT_DEGRADED`. The serializer has no source span, so attributing the note to a structure scope is the caller's job. |
| `bedrock_structure::BedrockStructureError` | `Nbt`, `AbstractPaletteEntry`, `State`, `PaletteIndexOutOfRange`, `DimensionOverflow`. |
| `bedrock_state::translate_states` | One Java blockstate → the Bedrock `states` compound, or the reason it cannot be one. |
| `bedrock_state::StateTranslation` | A successful translation: the typed `states` compound, plus one entry per piece of intent Bedrock cannot express. Empty `degraded` means lossless. |
| `bedrock_state::BedrockStateError` | `UnmappableBlock`, `UnknownStairState`, `UnknownStairKey` — each carrying the what-is-wrong / what-is-valid / suggested-fix triple the lint loop reads. |
| `data_version::JavaTarget` / `data_version::resolve_java_target` | `--target <mc_version>` → `(mc_version, DataVersion)`. |
| `data_version::BedrockTarget` / `data_version::resolve_bedrock_target` | `--target <mc_version>` → `(mc_version, block_version)`. |
| `data_version::UnsupportedTarget` | A `--target` no version table matched: which edition was consulted, the value given, the nearest candidate when one is close enough to suggest, and the supported list. |
| `data_version::supported_list` | That supported list on its own, pre-joined, so the CLI prints the same string the error does. |
| `portability::portability_for_java` | `PortabilityReport` for Java: how many palette entries are portable, degraded, or unsupported, and which. |
| `portability::portability_for_bedrock` | The same for Bedrock, or `InvalidPalette` when the palette carries states the pack should have refused. |
| `portability::PortabilityReport` | The counts and both entry lists, kept together so the figure and the list can never describe different palettes. Fields are private for that reason. |
| `portability::PortabilityCounts` | `portable` / `degraded` / `unsupported`, as `u32` palette-entry counts. |
| `portability::InvalidPalette` | Returned instead of counts when the palette is not one a build could have produced. Carries every refusal, unreworded, because they are one bug's symptoms. |
| `registry::builtin_java` / `registry::builtin_bedrock` | The block, alias, material, and data-version tables for one edition, built in and parsed once per process. |
| `registry::load_builtin_java` / `registry::load_builtin_bedrock` | The same two packs as a `Result`, for a caller that wants the parse error rather than the panic. |
| `registry::load_from_dir` | The same tables from an external pack directory. Loads it as a Java pack; there is no Bedrock equivalent yet. |
| `registry::RegistryPack` | A loaded and validated pack. `#[non_exhaustive]`, so "cannot be built without passing the validators" holds outside this crate too. |
| `registry::RegistryError` | Everything reading or validating a pack can refuse: a missing or unreadable component, a schema version past what this build understands, a failed validator. |
| `registry::PackSource` | `Builtin` or `Path`. Carried into diagnostics so a `--registry-pack` user can tell which directory was read. |
| `registry::PackManifest` | The `pack.json` body: schema version, edition, and the component file references. |
| `registry::PackEdition` | `java` / `bedrock`, closed, so `"BEDROCK"` cannot ride along as a valid pack. |
| `registry::PackFiles` | The component references inside a manifest. New components arrive as `Option` fields, so an older pack stays loadable. |

## Registry packs

A pack is the closed vocabulary every id-level answer comes from: which blocks exist in a version, which name a renamed block goes by there, and which canonical token an abstract material resolves to. `registry-data/<edition>/pack.json` holds the built-in pair, and `load_from_dir` takes an external one. Each table carries a `schema_version` the loader refuses to read past, so a pack written for a newer Cairn fails loudly instead of being half-understood.

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
