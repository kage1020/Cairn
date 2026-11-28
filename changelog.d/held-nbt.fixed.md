- *(formats)* The `.nbt`, `.mcstructure`, `.litematic` and `.schem` readers hold at most 64 MiB of
  the NBT they read whole of one file: its entities, its block entities' data, its palettes and its
  metadata. A file past that is refused by the path of the field that passes it, so a few kilobytes
  of gzip that unpack to millions of tags no longer exhaust the memory of `cairn import`,
  `cairn diff-blocks` or `cairn-lsp`. A `.nbt` or `.mcstructure` palette longer than a block array
  holds is refused from the length its list declares, before any entry of it is read.
- *(nbt)* `NbtReader::read_rest_within` reads the rest of the open compound or list as `read_rest`
  does, refused with `NbtReadError::HeldTooMuch` once the tags it builds would take more memory
  than its limit.
