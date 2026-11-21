- *(cli)* `W_SEMANTIC_SENSITIVITY` names only the members of this build whose boundary version
  lies between the lock's verified version and the new target, where it named every member the
  previous lock listed. It names each once with every reason and boundary it crosses
  (`box (item format at 1.20.5)`), and a member with no `id=` by its address.
- *(cli)* `cairn info`'s `semantic-sensitive` row is written per edition
  (`Java: (none)   Bedrock: (none)`), and each `--format json` `semantic_sensitive` entry gains
  `edition`.
- *(cli)* A lockfile's `constraint_catalog_hash` is the built-in catalog's sha256, where it was
  zeros, so a lock differs from the one an earlier release wrote for the same build.
- *(core)* `SemanticSensitiveFinding` gains `edition`, and `EditionReport` gains
  `semantic_sensitive`, which `compute_axes` collects into `VersionAxes::semantic_sensitive`.
