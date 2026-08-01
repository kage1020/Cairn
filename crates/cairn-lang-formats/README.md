# cairn-lang-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats.

Each format is just a serializer or de-serializer around the **block-array IR**, which the
specification names as the universal pivot
([architecture §3.1](https://cairn.kage1020.com/spec/architecture/)). Adding a new format means adding a backend here —
the parser, lint, and theme machinery in [`cairn-lang-core`](../cairn-lang-core/README.md) are untouched.

## Status

Java vanilla `.nbt` and Bedrock `.mcstructure` writers ship; the Bedrock
writer emits **stateless palettes only** — a palette entry carrying
blockstate properties is a hard error until per-edition state mapping
lands. Litematica `.litematic` and WorldEdit `.schem` are still to land.
No reverse-direction (file → IR) backends ship yet.

## Public API

| Item | Role |
|---|---|
| `java_structure::build_structure_tag` | `BlockArray` → `Compound` (Java vanilla shape). |
| `java_structure::write_structure_gzip` | Build + gzip-write in one call. |
| `java_structure::output_filename` | `struct::cottage` + `OutputExt::Nbt` → `cottage.nbt`; `OutputExt::Mcstructure` → `cottage.mcstructure`. |
| `java_structure::JavaStructureError` | `Nbt`, `AbstractPaletteEntry`, `DimensionOverflow`. |
| `bedrock_structure::build_mcstructure_tag` | `BlockArray` → `Compound` (Bedrock `.mcstructure` shape, stateless palette). |
| `bedrock_structure::write_mcstructure` | Uncompressed little-endian write of a built root. |
| `bedrock_structure::BedrockStructureError` | `Nbt`, `AbstractPaletteEntry`, `StatefulPaletteEntry`, `DimensionOverflow`. |
| `data_version::JavaTarget` / `resolve_java_target` | `--target <mc_version>` → `(mc_version, DataVersion)`. |
| `data_version::BedrockTarget` / `resolve_bedrock_target` | `--target <mc_version>` → `(mc_version, block_version)`. |

## Planned backends

| Format | Edition | Direction | Spec reference |
|---|---|---|---|
| `.nbt` (vanilla structure block) | Java | **write (done)**, read | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.mcstructure` | Bedrock | **write (stateless done)**, read | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.litematic` (Litematica) | Java | read / write | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/), [§12.4](https://cairn.kage1020.com/spec/ecosystem-interop/) |
| `.schem` (WorldEdit / Sponge) | Java | read / write | [ecosystem-interop §12.1](https://cairn.kage1020.com/spec/ecosystem-interop/) |

## Forward / reverse contract

- **Forward**: block-array IR → serialize. The compile pipeline writes the IR; each backend simply
  encodes it for one format.
- **Reverse**: deserialize → block-array IR + provenance stamp `(edition, version)`. The compiler
  performs only a *faithful transliteration* into the raw-centric DSL; semantic lifting is the LLM's
  job ([ecosystem-interop §12.2](https://cairn.kage1020.com/spec/ecosystem-interop/)). Litematica's multi-region
  structure is preserved as `site` placement, not flattened
  ([§12.4](https://cairn.kage1020.com/spec/ecosystem-interop/)).

## Out of scope

- Pre-1.13 legacy numeric-ID `.schematic`. v1 does not support flattening
  ([overview §1.3](https://cairn.kage1020.com/spec/overview/), [open-issues §15.1](https://cairn.kage1020.com/spec/open-issues/)).
- SNBT printing — Cairn does not round-trip through SNBT.

## Dependencies

- [`cairn-lang-core`](../cairn-lang-core/README.md) for the block-array IR types.
- [`cairn-lang-nbt`](../cairn-lang-nbt/README.md) for the byte-level codec.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
