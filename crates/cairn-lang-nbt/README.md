# cairn-lang-nbt

NBT codec for the Cairn language. The Java writer ships today; the Bedrock
codec and the streaming reader follow.

- **Java**: big-endian, gzipped, root compound tags. **Writer is public.**
- **Bedrock**: little-endian (and varint little-endian for network payloads),
  nameless lists of records. *Not yet implemented.*

This crate is deliberately *just* the codec. It does not know anything about Litematica regions,
schematic palettes, or Cairn's block-array IR — those live in
[`cairn-lang-formats`](../cairn-lang-formats/README.md). Keeping the byte layer separate means the codec can be
fuzzed and benchmarked without dragging in the higher-level format machinery.

## Status

Java writer ships. The full Java NBT tag taxonomy (`Byte` through
`LongArray`), an `IndexMap`-ordered `Compound`, and the two writer
entrypoints (`write_java_uncompressed` for tests, `write_java_gzip` for
the on-disk `.nbt` Minecraft expects) are public.

Bedrock little-endian and the streaming reader are still to land.

## Public API

| Item | Role |
|---|---|
| `tag::Tag` | Owned tag tree, one variant per NBT tag id (1..=12). |
| `tag::Compound` | `IndexMap<String, Tag>` — insertion order is the wire order. |
| `tag::List` | Homogeneous list with an explicit element type id. |
| `java::write_java_uncompressed` | Raw big-endian payload, no gzip. |
| `java::write_java_gzip` | Gzip-wrapped output at `Compression::default()`. |
| `java::NbtIoError` | `InvalidString`, `HeterogeneousList`, `LengthOverflow`, `Io`. |

## Scope

- Tag types: `End`, `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`,
  `Compound`, `IntArray`, `LongArray`.
- Both endiannesses, both flavors.
- A streaming reader for large files (Litematica regions, structure blocks split across many chunks).

Out of scope:

- SNBT parsing — Cairn never round-trips through SNBT
  ([overview §1.1](https://cairn.kage1020.com/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of the Cairn language semantics
  ([versioning-editions §10.2](https://cairn.kage1020.com/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
