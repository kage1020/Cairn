- *(cli)* `cairn install <file.nbt>... --world <world>` copies Java structure files
  `cairn compile` wrote into a world folder, where `/place template` and a structure block load
  them: `generated/<namespace>/structure/<name>.nbt`, or `structures/` in a world saved before
  26.1 Snapshot 6. It enforces cross-version application: a file stamped with a `DataVersion`
  newer than the one the world's `level.dat` records is `E_CROSS_VERSION_DOWNGRADE`, and one older
  is `E_CROSS_VERSION_UPGRADE` unless `--allow-cross-version` is given, which installs it byte for
  byte, keeping its stamp so the game's DataFixerUpper upgrades it, with
  `W_CROSS_VERSION_UPGRADE`. Either refusal names the `--target` to recompile at where one fits.
  `--namespace` (default `minecraft`) and `--name` set the structure id, which must be one the
  game loads and not end in `.nbt` (`E_INVALID_STRUCTURE_ID`); a file already there is
  `E_STRUCTURE_EXISTS` unless `--replace` is given, and so is a folder there or a file at its
  `.tmp` or `.bak` path, with or without it; a path the world cannot take is `E_WORLD_UNWRITABLE`;
  and `--dry-run` prints where each file would go and writes nothing. Every file is checked before
  any is written, a run reports each refusal it finds, and one that refuses a file installs none.
  A missing, unreadable or corrupted `level.dat` is `E_WORLD_UNREADABLE`, a file that is no Java
  structure `E_STRUCTURE_UNREADABLE`, and a Bedrock world or `.mcstructure`
  `E_INSTALL_EDITION_UNSUPPORTED`: a Bedrock world loads no structure from files in its folder.
  The subcommand and its codes are Evolving for their first three monthly minors.
- *(formats)* The `world` module reads what installing needs from a Java world:
  `read_level_dat` gives a world's `DataVersion` and release name, `read_structure_data_version`
  the stamp of a structure file without building its blocks, both checking the gzip checksum,
  `Namespace` and `StructureId` check an id against the game's path rules and name the file it
  loads from at the world's version, and `WorldVersion::cross_version` and `recompile_target` say
  how a structure stands to a world and which target to rebuild it at.
