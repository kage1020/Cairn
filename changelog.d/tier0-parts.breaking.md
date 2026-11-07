- *(core)* `repeater`, `comparator`, `redstone_torch` and `redstone_dust` are no longer
  `E_UNKNOWN_KEYWORD`, and an `@edition` guard holds them without `E_SEMANTIC_IN_EDITION_GUARD`.
- *(core)* `MemberRole` gains `Repeater`, `Comparator`, `RedstoneTorch` and `RedstoneDust`, so an
  exhaustive match on it no longer compiles. `MemberRole::is_raw` is now `is_escape_hatch`, true
  for a Tier 0 part too, beside the new `is_tier0_part`.
- *(core)* `JoinKind` gains `Dust` and `Joins` gains `dust`, so an exhaustive match or a struct
  literal of either no longer compiles.
- *(core)* A body whose members paint redstone dust writes it with its connection states rather
  than bare, so its palette, its `resolved_state` and the structure file differ from one built
  before.
- *(formats)* `SturdyRow` is now `FacesRow`, `ConnectionRow` gains `dust`, `ConnectionKind` gains
  `RedstoneDust`, and `VersionStates::java_ids` takes the entry's states. Both built-in packs'
  hashes move with the new tokens and connection rows.
