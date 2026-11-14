- *(core)* `sign` and `bed` are no longer `E_UNKNOWN_KEYWORD`.
- *(core)* `MemberRole` gains `Sign` and `Bed`, so an exhaustive match on it no longer compiles.
- *(core)* `BlockEntity` replaces its `id` and `fields` with `data`, a `BlockEntityData` that is
  `Fields { id, fields }`, as a circuit comparator's, `Sign { front, back }` of two `SignText`, or
  `Bed { color }`. `cairn lower --format json` writes each block entity with a `kind`, `fields`,
  `sign` or `bed`, beside its `pos`. `block_array` also gains `SignText` and `DYE_COLORS`.
- *(formats)* `StateTranslation` gains `entity`, a `StateEntity`; `BedrockForm` gains `in_entity`,
  an `EntityStates`; `SensitivityKind` gains `BlockEntity`; and `MaterialEntry` gains `canonical`.
  Each breaks a struct literal or an exhaustive match.
- *(formats)* `JavaStructureError` gains `UnencodableSignText`, and `BedrockStructureError` gains
  `UnknownBedColor` and `UnencodableSignText`.
- *(formats)* A Bedrock build no longer reports a chest pair or a note block's `note` as
  `W_INTENT_DEGRADED`: the Bedrock pack classifies both as `block_entity`, and both built-in packs'
  hashes move with that and the new `bed` rows.
