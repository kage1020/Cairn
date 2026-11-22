- *(core)* The structured entities are member keywords. `painting` and `item_frame` hang on a
  wall's face, placed by `at=`, `offset=` and `y=` as a `chest` is, or at a coordinate triple on
  the face `facing=` names; a painting runs right and up from that cell over the size its
  `variant=` has at the target. `armor_stand`, `villager` and the displays `text_display`,
  `block_display` and `item_display` stand by a wall facing away from it, or at a coordinate triple
  facing south or as `facing=` names. A `villager` takes `profession=` and `level=`, and each
  `trade` line names a villager's `id=` and adds an offer: `buy=` and `count=` for `sell=` and
  `sell_count=`, the item sold enchanted by `enchant=` and `enchant_level=`. A `text_display`
  shows its `text=` and takes `billboard=` and `scale=`, a `block_display` shows the block its
  `mat_slot=` resolves to, and an `item_display` the `item=` it names.
- *(core)* `spawn type=` places any other entity by its id. It is of the escape-hatch layer, so
  it stands under an `@edition` guard, and so does an entity's line that writes `nbt={...}`,
  whose top-level keys the writer puts over the fields it derives. A `raw_block` may write one for
  its block's block entity. A compound outside a guard is the new error
  `E_NBT_OUTSIDE_EDITION_GUARD`.
- *(core)* A theme's `slot` row may bind a member form, `text_display` with its `billboard=` and
  `scale=` or `sign` with its `glowing=`, and `hologram` is floating text built as the form its
  `mat_slot=` binds: a Java theme can build it as a text display and a Bedrock theme as a glowing
  sign. A display on Bedrock, or a hologram built as one, is `E_PARITY_UNSUPPORTED` with the
  Bedrock pack's reason, and a member form bound to a member other than a `hologram` is
  `E_INVALID_MEMBER`.
- *(core)* A build pinned to a target warns with the new `W_ENTITY_CLIPPING` on an entity whose
  box, from the constraint catalog, reaches into a full block, a door's cells or the box of an
  entity an earlier line places, and with `W_NO_ATTACHMENT_FACE` on a painting or item frame with
  air, water or lava behind a cell it covers. `TargetRegistry` gains `solid`, `entity_box`,
  `painting` and `absent_entity`, each with a default that answers nothing.
- *(formats)* The `.nbt` writer saves each entity in `entities` with its `pos`, `blockPos` and
  `nbt`, spelling an item, a display's text and its block state as the target's `DataVersion`
  reads them. The `.mcstructure` writer saves each in its `entities`, and builds an item frame as
  the frame block with its block entity.
- *(formats)* Both built-in packs carry `paintings.json`, each targetable version's paintings
  with their sizes in blocks, read through `RegistryPack::paintings`. Bedrock's `absent.json`, at
  schema 2, lists the display entities, and the constraint catalog's `entity_aabb` gives a box to
  every entity a member places.
- *(lsp)* Completion offers the ten keywords, and under an `@edition` guard `spawn` and the
  entities whose line may write `nbt=`.
- *(vscode)* The grammar highlights the ten keywords.
- *(tree-sitter)* A `slot` row that binds a bare word takes `key=value` arguments after it.
- `examples/trading-post.crn` furnishes a trading post with a villager and its trades, a painting,
  an item frame, an armor stand and a hologram each edition builds its own way.
