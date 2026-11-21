- *(cli)* `cairn import FILE` reads a `.nbt`, `.litematic`, `.schem` or `.mcstructure` file and
  writes `<stem>.raw.crn` beside it, or the path `--out` names: a `.crn` of raw members that
  compiles back to the blocks the file holds, so a structure saved by another tool becomes a source
  to lift. The source is stamped with the shipped target the file was saved at, or the nearest after
  it, in its header, `@requires` and `@intended_targets`, and the note printed last names the
  `cairn compile` that builds it. Every run warns with `W_SEMANTIC_LOSS` that it recovers blocks and
  their regularity rather than intent, then once for each kind of thing no raw member carries: block
  entity data, entities, structure voids, a `.nbt`'s extra palettes, and on Bedrock what the second
  layer holds and what `block_position_data` holds besides block entity data. A file saved past
  every shipped target, one the target cannot build back cell for cell, one whose blocks take more
  palette entries than a block array holds, and one holding a block no `mat=` can spell are refused
  with exit 1 and nothing written; an extension no format has, a missing file and an `--out` naming
  the file itself are exit 2.
- *(core)* `raw_repeat count=N step=dx,dy,dz` writes the `raw_block` and `raw_fill` lines indented
  under it again at each step, under a `level` or an `@edition` guard too. Each member is judged by
  its last copy, overlapping copies of one member raise no `W_PHASE_CONFLICT`, any other line under
  it is `E_UNSUPPORTED_NESTING`, and a missing or unreadable `count=` or `step=`, `step=0,0,0` over
  more than one copy, or a repeat with no raw member under it is `E_INVALID_MEMBER`.
- *(core)* `origin=imported` on a `raw_block` or `raw_fill` marks a line an import wrote: it sizes
  the struct's height, and its block takes the connection states its `mat=` leaves out from the
  blocks beside it. Any other value is `E_INVALID_MEMBER`.
- *(core)* `transliterate` writes block grids as a `.crn` of raw members: boxes grown along `x`,
  then `z`, then `y`, runs of equal boxes folded into `raw_repeat`, connection states left to the
  rules, and a body wider or deeper than 48 blocks split into a `site` of 48 by 48 chunks. It checks
  what it writes by lowering it back and comparing every cell.
- *(formats)* `read_structure` reads a Java `.nbt` and `read_mcstructure` a Bedrock `.mcstructure`,
  streaming as the `.litematic` and `.schem` readers do and refusing what they refuse: a list or
  array longer than the file could need before it is read, input past the root, and on a `.nbt` a
  gzip trailer that does not match and a field of the root stored twice. A `.nbt` is read from the
  first of its `palettes` when it has them, as the game reads it. A `BedrockVolume`, like a
  `BlockVolume`, is built only through `new`, which holds each voxel to its palette and each block
  entity to its box. `import::decode_structure` turns any of the four into block grids stamped with
  a target, each loss tagged with a `LossKind`, and `import::import_raw` writes them as the source
  `cairn import` writes.
- *(lsp)* Completion on a line under a `raw_repeat` offers only `raw_block` and `raw_fill`.
