- *(cli)* `cairn compile --structure-format schem` writes Java's artifacts as Sponge Schematic
  version 3 `.schem` files, which WorldEdit loads from 7.3.0: one per struct, placement and walkway,
  named as the `.nbt` files are, each with its origin in its site as its `Offset`, so pasting a
  site's files at one point rebuilds the site.
- *(formats)* The `.schem` writer streams the varint block data from the grid, writes block
  entities as the `.nbt` writer does, and leaves out the save date, so a source always compiles to
  the same bytes. A `.schem` reader takes versions 2 and 3 and returns a `BlockVolume`.
- *(nbt)* `CompoundStream::byte_array` writes a byte array from an iterator without collecting it.
