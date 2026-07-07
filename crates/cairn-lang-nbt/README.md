# cairn-lang-nbt

NBT codec for the Cairn language. The Java and Bedrock writers ship today;
the streaming reader follows.

- **Java**: big-endian, gzipped, root compound tags. **Writer is public.**
- **Bedrock**: little-endian, uncompressed (the `.mcstructure` on-disk
  form). **Writer is public.** The varint little-endian network payload
  form is not needed for structure files and has not landed.

This crate is deliberately *just* the codec. It does not know anything about Litematica regions,
schematic palettes, or Cairn's block-array IR — those live in
[`cairn-lang-formats`](../cairn-lang-formats/README.md). Keeping the byte layer separate means the codec can be
fuzzed and benchmarked without dragging in the higher-level format machinery.

## Status

Both writers ship. The full NBT tag taxonomy (`Byte` through `LongArray`),
an `IndexMap`-ordered `Compound`, and the writer entrypoints are public.
The byte-level encoder is a single endian-parameterised core, so the Java
and Bedrock writers share validation rules and cannot drift apart.

The streaming reader is still to land.

## Public API

| Item | Role |
|---|---|
| `tag::Tag` | Owned tag tree, one variant per NBT tag id (1..=12). |
| `tag::Compound` | `IndexMap<String, Tag>` — insertion order is the wire order. |
| `tag::List` | Homogeneous list with an explicit element type id. |
| `java::write_java_uncompressed` | Raw big-endian payload, no gzip. |
| `java::write_java_gzip` | Gzip-wrapped big-endian output at `Compression::default()`. |
| `bedrock::write_bedrock_uncompressed` | Raw little-endian payload (the `.mcstructure` form). |
| `java::NbtIoError` | `InvalidString`, `HeterogeneousList`, `LengthOverflow`, `Io`. |

## Scope

- Tag types: `End`, `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`,
  `Compound`, `IntArray`, `LongArray`.
- Both endiannesses (big-endian Java, little-endian Bedrock) ship on the writer side.
- A streaming reader for large files (Litematica regions, structure blocks split across many chunks).

Out of scope:

- SNBT parsing — Cairn never round-trips through SNBT
  ([overview §1.1](https://cairn.kage1020.com/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of the Cairn language semantics
  ([versioning-editions §10.2](https://cairn.kage1020.com/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
