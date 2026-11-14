- *(cli)* `W_SEMANTIC_SENSITIVITY` names the members of this build whose boundary version lies
  between the lock's verified version and the new target, each with its reason and boundary
  (`box (item format at 1.20.5)`), rather than every member the previous lock listed.
- *(cli)* `cairn info`'s `semantic-sensitive` row is written per edition
  (`Java: (none)   Bedrock: (none)`), and each `--format json` `semantic_sensitive` entry gains
  `edition`.
- *(cli)* A lockfile's `constraint_catalog_hash` is the built-in catalog's sha256 rather than
  zeros, so a lock differs from the one an earlier release wrote for the same build.
- *(core)* `SemanticSensitiveFinding` gains `edition`, and `EditionReport` gains
  `semantic_sensitive`, which `compute_axes` collects into `VersionAxes::semantic_sensitive`.
