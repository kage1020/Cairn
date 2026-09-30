# cairn-lang-nbt

NBT codec for the Cairn language, in both on-disk dialects.

- **Java**: big-endian, gzip-wrapped, root compound tags.
- **Bedrock**: little-endian, uncompressed — the `.mcstructure` on-disk form. The varint little-endian network payload is a different shape, is not needed for structure files, and has not landed.

This crate is deliberately *just* the codec. It knows nothing about Litematica regions, schematic palettes, or Cairn's block-array IR — those live in [`cairn-lang-formats`](../cairn-lang-formats/README.md). Keeping the byte layer separate means it can be fuzzed and benchmarked without dragging in the higher-level format machinery. The CLI never reaches in directly; it talks to the format helpers, which talk to this crate.

## Status

Both writers ship. The full tag taxonomy (`Byte` through `LongArray`), an `IndexMap`-ordered `Compound`, and the writer entry points are public. The byte-level encoder is a single endian-parameterised core, so the Java and Bedrock writers share their validation rules and cannot drift apart.

Each writer also has a streaming form, which writes a root compound entry by entry instead of from a built tree: a list declares its length up front and takes its items one at a time, so a list with an entry per voxel never has to be in memory. It goes through the same encoder core, so a streamed root is the bytes the tree writer writes for the same tags.

The streaming reader is still to land. It is what the reverse direction needs — reading large files such as Litematica regions and structure blocks split across many chunks — so no `.nbt` → IR path exists until it does.

## Public API

Every item the crate root re-exports, and nothing else — the same rule [`cairn-lang-formats`](../cairn-lang-formats/README.md) states, held by the same test in both directions.

| Item | Role |
|---|---|
| `tag::Tag` | Owned tag tree, one variant per NBT tag id (1..=12). |
| `tag::Compound` | `IndexMap<String, Tag>` — insertion order is the wire order. |
| `tag::List` | Homogeneous list with an explicit element type id. |
| `java::write_java_uncompressed` | Raw big-endian payload, no gzip. |
| `java::write_java_gzip` | Gzip-wrapped big-endian output at `Compression::default()`. |
| `bedrock::write_bedrock_uncompressed` | Raw little-endian payload (the `.mcstructure` form). |
| `java::stream_java_uncompressed` / `java::stream_java_gzip` / `bedrock::stream_bedrock_uncompressed` | Streaming twins of the three writers: a closure writes the root's entries through a `CompoundStream`. Same bytes as the tree writer for the same tags. |
| `stream::CompoundStream` | A compound being written: `tag` appends a named tag, `compound` and `list` open a nested one. `TAG_End` is written when the closure returns. |
| `stream::ListStream` | A list being written after its declared length: `item`, `compound` and `list` append one item each. Writing a number of items other than the declared length panics, since the prefix is already on the wire. |
| `java::NbtIoError` | `InvalidString`, `HeterogeneousList`, `EmptyListWithElementType`, `LengthOverflow`, `Io`. |

Tag types covered: `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`, `Compound`, `IntArray`, `LongArray`. `TAG_End` has no variant — it is implicit in `Compound` termination, so a caller cannot construct a stray end marker.

## Out of scope

- SNBT parsing — Cairn never round-trips through SNBT ([overview "Purpose"](https://cairn.kage1020.com/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of Cairn's language semantics ([versioning-editions "Language contract: recompile, don't transcode"](https://cairn.kage1020.com/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
