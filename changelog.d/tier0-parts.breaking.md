- *(core)* `repeater`, `comparator`, `redstone_torch` and `redstone_dust` are no longer
  `E_UNKNOWN_KEYWORD`, and an `@edition` guard holds them without `E_SEMANTIC_IN_EDITION_GUARD`.
- *(core)* `MemberRole` gains `Repeater`, `Comparator`, `RedstoneTorch` and `RedstoneDust`, so an
  exhaustive match on it no longer compiles. `MemberRole::is_raw` is now `is_escape_hatch`, true
  for a Tier 0 part too, beside the new `is_tier0_part`.
- *(core)* `JoinKind` gains `Dust` and `Joins` gains `dust`, so an exhaustive match or a struct
  literal of either no longer compiles.
- *(core)* `BlockArray` gains `derived`, per palette slot the state keys the lowering derived, so
  a struct literal of it no longer compiles.
- *(core)* A body whose members paint redstone dust writes it with its connection states rather
  than bare, so its palette, its `resolved_state` and the structure file differ from one built
  before.
- *(formats)* `ConnectionRow` gains `dust` and `ConnectionKind` gains `RedstoneDust`, so a struct
  literal or an exhaustive match of either no longer compiles. `VersionStates::java_ids` and
  `resolve` read an id both editions spell, such as `redstone_torch`, as the Java block that takes
  the entry's states. Both built-in packs' hashes move with the new tokens and connection rows.
- *(formats)* `translate_states` takes the keys the lowering derived for the entry and
  `BedrockForm` gains `rederived`, so a call or a struct literal no longer compiles. Over an array
  whose `derived` is empty, a Bedrock build and the portability report read every `derived` state
  an entry carries as written, and warn of it.
- *(formats)* With no version pinned, a block is read as each thing every version declaring it
  agrees on, its kind, its whole faces, what its bottom covers and the faces dust joins, where it
  was read as nothing once the versions disagreed on any one. So a standing redstone torch, whose
  bottom covers differ between versions, still joins dust, and an unpinned `cairn lower` or
  editor may derive connection states it left out before.
