# cairn-nbt

NBT codec for the Cairn language. Encodes and decodes Minecraft NBT in both flavors:

- **Java**: big-endian, gzipped, root compound tags.
- **Bedrock**: little-endian (and varint little-endian for network payloads), nameless lists of
  records.

This crate is deliberately *just* the codec. It does not know anything about Litematica regions,
schematic palettes, or Cairn's block-array IR — those live in
[`cairn-formats`](../cairn-formats/README.md). Keeping the byte layer separate means the codec can be
fuzzed and benchmarked without dragging in the higher-level format machinery.

## Status

Skeleton. No public types are exposed yet; the reader/writer for both endiannesses, gzip framing for
Java, and the tag-type taxonomy are still to land.

## Scope

- Tag types: `End`, `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`,
  `Compound`, `IntArray`, `LongArray`.
- Both endiannesses, both flavors.
- A streaming reader for large files (Litematica regions, structure blocks split across many chunks).

Out of scope:

- SNBT parsing — Cairn never round-trips through SNBT
  ([overview §1.1](https://kage1020.github.io/Cairn/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of the Cairn language semantics
  ([versioning-editions §10.2](https://kage1020.github.io/Cairn/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
