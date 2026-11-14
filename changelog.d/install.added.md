- *(cli)* `cairn install <file.nbt>... --world <world>` copies Java structure files
  `cairn compile` wrote into a world folder, at `generated/<namespace>/structures/<name>.nbt`,
  where `/place template` and a structure block load them. It enforces cross-version application:
  a file stamped with a `DataVersion` newer than the one the world's `level.dat` records is
  `E_CROSS_VERSION_DOWNGRADE`, and one older is `E_CROSS_VERSION_UPGRADE` unless
  `--allow-cross-version` is given, which installs it byte for byte, keeping its stamp so the
  game's DataFixer upgrades it, with `W_CROSS_VERSION_UPGRADE`. Each refusal names the `--target`
  to recompile at. `--namespace` (default `minecraft`) and `--name` set the structure id, which
  must be one the game loads (`E_INVALID_STRUCTURE_ID`); a file already there is
  `E_STRUCTURE_EXISTS` unless `--replace` is given; and `--dry-run` prints where each file would go
  and writes nothing. Every file is checked before any is written, and a run that refuses one
  installs none. A missing or unreadable `level.dat` is `E_WORLD_UNREADABLE`, a file that is no
  Java structure `E_STRUCTURE_UNREADABLE`, and a Bedrock world or `.mcstructure`
  `E_INSTALL_EDITION_UNSUPPORTED`: a Bedrock world loads no structure from files in its folder.
  The subcommand and its codes are Evolving for their first three monthly minors.
- *(formats)* The `world` module reads what installing needs from a Java world:
  `read_level_dat` gives a world's `DataVersion` and release name, `read_structure_data_version`
  the stamp of a structure file without building its blocks, `StructureId` checks an id against
  the game's path rules and names the file it loads from, and `CrossVersion` and
  `recompile_target` say how a structure stands to a world and which target to rebuild it at.
- *(nbt)* NBT readers: `read_java_uncompressed`, `read_java_gzip` and `read_bedrock_uncompressed`
  build the root entries a `ReadOptions` keeps and skip the rest, within a byte budget, refusing
  nesting past the game's 512 levels, negative lengths, untyped lists and strings that are not the
  dialect's encoding with an `NbtReadError`.
