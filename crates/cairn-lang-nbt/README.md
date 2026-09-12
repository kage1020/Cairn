# cairn-lang-nbt

NBT codec for the Cairn language, in both on-disk dialects.

- **Java**: big-endian, gzip-wrapped, root compound tags.
- **Bedrock**: little-endian, uncompressed — the `.mcstructure` on-disk form. The varint little-endian network payload is a different shape, is not needed for structure files, and has not landed.

This crate is deliberately *just* the codec. It knows nothing about Litematica regions, schematic palettes, or Cairn's block-array IR — those live in [`cairn-lang-formats`](../cairn-lang-formats/README.md). Keeping the byte layer separate means it can be fuzzed and benchmarked without dragging in the higher-level format machinery. The CLI never reaches in directly; it talks to the format helpers, which talk to this crate.

## Status

Both writers ship. The full tag taxonomy (`Byte` through `LongArray`), an `IndexMap`-ordered `Compound`, and the writer entry points are public. The byte-level encoder is a single endian-parameterised core, so the Java and Bedrock writers share their validation rules and cannot drift apart.

The streaming reader is still to land. It is what the reverse direction needs — reading large files such as Litematica regions and structure blocks split across many chunks — so no `.nbt` → IR path exists until it does.

## Public API

| Item | Role |
|---|---|
| `tag::Tag` | Owned tag tree, one variant per NBT tag id (1..=12). |
| `tag::Compound` | `IndexMap<String, Tag>` — insertion order is the wire order. |
| `tag::List` | Homogeneous list with an explicit element type id. |
| `java::write_java_uncompressed` | Raw big-endian payload, no gzip. |
| `java::write_java_gzip` | Gzip-wrapped big-endian output at `Compression::default()`. |
| `bedrock::write_bedrock_uncompressed` | Raw little-endian payload (the `.mcstructure` form). |
| `java::NbtIoError` | `InvalidString`, `HeterogeneousList`, `EmptyListWithElementType`, `LengthOverflow`, `Io`. |

Tag types covered: `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`, `Compound`, `IntArray`, `LongArray`. `TAG_End` has no variant — it is implicit in `Compound` termination, so a caller cannot construct a stray end marker.

## Out of scope

- SNBT parsing — Cairn never round-trips through SNBT ([overview §1.1](https://cairn.kage1020.com/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of Cairn's language semantics ([versioning-editions §10.2](https://cairn.kage1020.com/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
