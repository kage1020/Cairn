- *(core)* `sign` and `bed` are no longer `E_UNKNOWN_KEYWORD`.
- *(core)* `MemberRole` gains `Sign` and `Bed`, so an exhaustive match on it no longer compiles.
- *(core)* `BlockEntity` replaces its `id` with `data`, a `BlockEntityData` that is
  `Fields { id, fields }`, as a circuit comparator's, `Sign { front, back }` of two `SignText`, or
  `Bed { color }` of an `Option<DyeColor>`. `cairn lower --format json` writes each block entity
  with a `kind`, `fields`, `sign` or `bed`, beside its `pos`.
- *(formats)* `StateTranslation` gains `entity`, an `Option<StateEntity>`; `BedrockForm` gains
  `in_entity`, an `Option<EntityStates>`; `SensitivityKind` gains `BlockEntity`; and
  `MaterialEntry` gains `canonical`. Each breaks a struct literal or an exhaustive match.
- *(formats)* `BedrockStructureError::UnmappedBlockEntity` gains `pos`, the block entity's voxel.
- *(formats)* Both built-in packs' hashes move with the Bedrock pack's `block_entity` rows and the
  new `bed` rows.
