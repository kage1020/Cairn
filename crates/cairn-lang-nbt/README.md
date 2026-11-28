# cairn-lang-nbt

This crate's Rust API is in the [Internal tier](https://cairn.kage1020.com/spec/compatibility/#internal) and carries no promise: any release may change it.

NBT codec for the Cairn language, in both on-disk dialects.

- **Java**: big-endian, gzip-wrapped, root compound tags.
- **Bedrock**: little-endian, uncompressed — the `.mcstructure` on-disk form. The varint little-endian network payload is a different shape, is not needed for structure files, and has not landed.

This crate is deliberately *just* the codec. It knows nothing about Litematica regions, schematic palettes, or Cairn's block-array IR — those live in [`cairn-lang-formats`](../cairn-lang-formats/README.md). Keeping the byte layer separate means it can be fuzzed and benchmarked without dragging in the higher-level format machinery. The CLI never reaches in directly; it talks to the format helpers, which talk to this crate.

## Status

Both writers and the reader ship. The full tag taxonomy (`Byte` through `LongArray`), an `IndexMap`-ordered `Compound`, and the writer entry points are public. The byte-level encoder is a single endian-parameterised core, so the Java and Bedrock writers share their validation rules and cannot drift apart.

Each uncompressed writer also has a streaming form, which writes a root compound entry by entry instead of from a built tree: a list declares its length up front and takes its items one at a time, so a list with an entry per voxel never has to be in memory. It goes through the same encoder core, so a streamed root is the bytes the tree writer writes for the same tags.

The reader is a pull parser over either dialect: `NbtReader` yields a root's tags one event at a time, in wire order, so a caller reading a structure's per-voxel `blocks` list takes one entry at a time with `read_rest`, and passes over what it does not need with `skip_rest` without holding it. The tree readers build on it. It refuses what is not NBT with the byte offset where the fault starts: a type id past 12, a negative or over-limit length, a non-empty list of `TAG_End`, nesting past the game's 512, a string its dialect cannot decode (Modified UTF-8 on Java, UTF-8 on Bedrock), input that ends before the root closes, or input past the root. A root that is not a compound is refused too, at its first byte. Once a call fails, every later call fails, since the bytes after a fault no longer line up with tags. A declared length reserves memory only as its bytes arrive, so a false one cannot exhaust it up front. `finish` asks the input for one byte past the root, which is also what makes a gzip decoder check its trailer; the gzip tree reader decodes gzip members after the first as one stream, as Java does, so a second member is input past the root.

## Public API

Every item the crate root re-exports, and nothing else — the same rule [`cairn-lang-formats`](../cairn-lang-formats/README.md) states, held by the same check in both directions.

| Item | Role |
|---|---|
| `tag::Tag` | Owned tag tree, one variant per NBT tag id (1..=12). |
| `tag::Compound` | `IndexMap<String, Tag>` — insertion order is the wire order. |
| `tag::List` | Homogeneous list with an explicit element type id. |
| `java::write_java_uncompressed` | Raw big-endian payload, no gzip. |
| `java::write_java_gzip` | Gzip-wrapped big-endian output at `Compression::default()`. |
| `bedrock::write_bedrock_uncompressed` | Raw little-endian payload (the `.mcstructure` form). |
| `java::stream_java_uncompressed` / `bedrock::stream_bedrock_uncompressed` | Streaming twins of the two uncompressed writers: a closure writes the root's entries through a `CompoundStream`. Same bytes as the tree writer for the same tags. For gzip, wrap the writer in an encoder at `Compression::default()`; no `flate2` type is in this crate's signatures. |
| `stream::CompoundStream` | A compound being written: `tag` appends a named tag, `compound_tag` a compound the caller holds, `compound` and `list` open a nested one, and `long_array` and `byte_array` write a `TAG_Long_Array` or `TAG_Byte_Array` from an iterator, so a packed array never has to be in memory. `TAG_End` is written when the closure returns successfully. A key named twice is written twice. An array given a number of values other than its declared length panics. |
| `stream::ListStream` | A list being written after its declared length: `item`, `compound` and `list` append one item each. An empty list declares `TAG_End`, as `List::of_tags` does. Writing a number of items other than the declared length panics; a wrong item type is refused before any of its bytes, so it returns `HeterogeneousList` instead. |
| `java::check_java_string` / `bedrock::check_bedrock_string` | Whether a string can be written as an NBT string by that edition's writers, ahead of the write: every character can, and the error is `LengthOverflow` for one whose encoding, Modified UTF-8 on Java and UTF-8 on Bedrock, runs past 65,535 bytes. |
| `java::NbtIoError` | `HeterogeneousList`, `EmptyListWithElementType`, `LengthOverflow`, `Io`. `#[non_exhaustive]`. |
| `read::NbtReader` | The pull reader: `java` or `bedrock` over an uncompressed stream (wrap gzip in a decoder first), `with_max_len` to refuse longer lists and arrays and `with_max_array_len` to hold arrays to a tighter limit, `next_event`, then `read_rest` or `skip_rest` for the rest of the innermost open compound or list. `depth` and `offset` say where it is. `finish`, once the root has closed, refuses input past it and returns the input. |
| `read::Event` | `Compound`, `List` (with its element type and length; an empty list reads as one of `TAG_End`), `Value` for every other tag, arrays included, and `End`. A list item's `name` is `None`. |
| `read::MAX_DEPTH` | 512, the deepest nesting of compounds and lists the reader accepts. |
| `read::read_java_uncompressed` / `read::read_java_gzip` / `read::read_bedrock_uncompressed` | One named root read whole as `(name, Compound)`, through `finish`, so input past it is refused, and on gzip a trailer that does not match. |
| `read::NbtReadError` | `UnknownTagType`, `NegativeLength`, `ListOfEnd`, `TooLong`, `TooDeep`, `InvalidString`, `Truncated` and `TrailingBytes`, each with its offset into the uncompressed stream; `RootNotCompound`, always at the first byte; `AfterFailure`, for a call after one that failed, with where the reader stopped; and `Io`, a failure of the stream beneath, gzip included. `#[non_exhaustive]`. |

Tag types covered: `Byte`, `Short`, `Int`, `Long`, `Float`, `Double`, `ByteArray`, `String`, `List`, `Compound`, `IntArray`, `LongArray`. `TAG_End` has no variant — it is implicit in `Compound` termination, so a caller cannot construct a stray end marker.

## Out of scope

- SNBT parsing — Cairn never round-trips through SNBT ([overview "Purpose"](https://cairn.kage1020.com/spec/overview/)).
- DataFixerUpper-style version migration. DFU is explicitly kept out of Cairn's language semantics ([versioning-editions "Language contract: recompile, don't transcode"](https://cairn.kage1020.com/spec/versioning-editions/)).

## License

Apache-2.0. See [LICENSE](../../LICENSE).
