# cairn-formats

Readers and writers around the Cairn block-array IR for existing schematic / structure formats.

Each format is just a serializer or de-serializer around the **block-array IR**, which the
specification names as the universal pivot
([architecture §3.1](https://kage1020.github.io/Cairn/spec/architecture/)). Adding a new format means adding a backend here —
the parser, lint, and theme machinery in [`cairn-core`](../cairn-core/README.md) are untouched.

## Status

Skeleton. The crate compiles, but no format backends are implemented yet.

## Planned backends

| Format | Edition | Direction | Spec reference |
|---|---|---|---|
| `.nbt` (vanilla structure block) | Java | read / write | [ecosystem-interop §12.1](https://kage1020.github.io/Cairn/spec/ecosystem-interop/) |
| `.litematic` (Litematica) | Java | read / write | [ecosystem-interop §12.1](https://kage1020.github.io/Cairn/spec/ecosystem-interop/), [§12.4](https://kage1020.github.io/Cairn/spec/ecosystem-interop/) |
| `.schem` (WorldEdit / Sponge) | Java | read / write | [ecosystem-interop §12.1](https://kage1020.github.io/Cairn/spec/ecosystem-interop/) |
| `.mcstructure` | Bedrock | read / write | [ecosystem-interop §12.1](https://kage1020.github.io/Cairn/spec/ecosystem-interop/) |

## Forward / reverse contract

- **Forward**: block-array IR → serialize. The compile pipeline writes the IR; each backend simply
  encodes it for one format.
- **Reverse**: deserialize → block-array IR + provenance stamp `(edition, version)`. The compiler
  performs only a *faithful transliteration* into the raw-centric DSL; semantic lifting is the LLM's
  job ([ecosystem-interop §12.2](https://kage1020.github.io/Cairn/spec/ecosystem-interop/)). Litematica's multi-region
  structure is preserved as `site` placement, not flattened
  ([§12.4](https://kage1020.github.io/Cairn/spec/ecosystem-interop/)).

## Out of scope

- Pre-1.13 legacy numeric-ID `.schematic`. v1 does not support flattening
  ([overview §1.3](https://kage1020.github.io/Cairn/spec/overview/), [open-issues §15.1](https://kage1020.github.io/Cairn/spec/open-issues/)).
- SNBT printing — Cairn does not round-trip through SNBT.

## Dependencies

- [`cairn-core`](../cairn-core/README.md) for the block-array IR types.
- [`cairn-nbt`](../cairn-nbt/README.md) for the byte-level codec.

## License

Apache-2.0. See [LICENSE](../../LICENSE).
