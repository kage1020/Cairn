- *(cli)* `cairn compile --structure-format litematic` writes Java's artifacts as Litematica
  `.litematic` schematics: a struct to a file of its own, and a site to one file named after it,
  with a region per placement and walkway at its site origin, so Litematica loads a whole site as
  one placement. `nbt` stays the default. The naming rule, the collision guards and the lockfile
  are the same whichever format is written, and a site named after a struct is refused, as the one
  file both would write. Bedrock has one structure format, so the flag with `--edition bedrock`
  exits 2 before the file is read.
- *(formats)* The `.litematic` writer stamps `Version` 6 for 1.20.4 and 7 from 1.20.5, the version
  a Litematica for each release reads, writes block entities as the `.nbt` writer does, and leaves
  out the save times and preview image, so a source always compiles to the same bytes. A
  `.litematic` reader returns each region's voxels, palette and block entities, holding one region
  in memory at a time.
- *(nbt)* `NbtReader`, a pull reader for both dialects, yields a root one tag at a time and refuses
  malformed input with the byte offset where the fault starts, and `CompoundStream::long_array`
  writes a long array from an iterator without collecting it.
