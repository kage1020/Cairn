- *(core)* `painting`, `item_frame`, `armor_stand`, `villager`, `trade`, `text_display`,
  `block_display`, `item_display`, `hologram` and `spawn` are no longer `E_UNKNOWN_KEYWORD`, and a
  `slot` row bound to `text_display` or `sign` is no longer `E_UNKNOWN_SLOT_TARGET`.
- *(core)* `MemberRole` gains a variant for each of the ten keywords, so an exhaustive match on it
  no longer compiles.
- *(core)* `block_array::Entity` replaces its `pos` and `id` with `id`, `pos`, `block_pos`,
  `facing`, `data`, an `EntityData`, and `nbt`, and `BlockEntityData` gains `Nbt`.
  `cairn lower --format json` writes the entities a struct places. `block_array` also gains
  `EntityData`, `Item`, `Enchantment`, `Trade`, `EntityBox`, `PaintingLookup`, `PaintingVariant`
  and `AbsentEntity`.
- *(core)* `ThemeRule::Slot` gains `arguments`, and `ThemeBinding` and `ScopeResolution` gain
  `slot_arguments`. Each breaks a struct literal or a pattern without `..`.
- *(formats)* `JavaStructureError` gains `UnencodableEntity` and `MalformedNbt`.
  `BedrockStructureError` gains `UnmappedEntity`, `FrameCellTaken`, `FrameOutsideGrid`,
  `UnmappedEnchantment`, `UnencodableEntity` and `MalformedNbt`.
- *(formats)* `PackFiles` gains `paintings`, `AbsentCatalog` gains `entities`, `RegistryError`
  gains `Paintings`, and `SUPPORTED_ABSENT_SCHEMA` is 2.
- *(formats)* Both built-in packs' hashes move with `paintings.json` and Bedrock's absent
  entities, so a lock written before records another `registry_pack_hash`.
