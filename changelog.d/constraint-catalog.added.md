- *(formats)* The first constraint catalog ships beside the registry packs as
  `registry-data/constraints.json`: one hand-written file for both editions, in canonical ids. It
  lists the blocks that hang on a face (`attachment`), stand on or hang from one (`support`) and
  fall (`gravity`), the faces besides whole ones and posts that hold a centre (`center_support`),
  the boxes of entities (`entity_aabb`), and the Java versions at which a block's meaning changes
  (`semantic_sensitivity`: wall connections at 1.16, the cauldron split at 1.17, the item format
  at 1.20.5). It is checked against both packs as it loads, and a key it does not know, a key it
  requires and lacks, or two rows that give one state different defaults are refused.
  `builtin_constraints`, `load_builtin_constraints` and `parse_constraints` load it,
  `PackView::with_constraints` hands it to a view, and `builtin_view` is the built-in pack's view
  of an edition at a version, or at none, with it attached.
- *(core)* A build pinned to a target warns on a block the game would drop or let fall, and on
  water that would run out of it. `W_NO_ATTACHMENT_FACE` is a lever, button, wall torch, ladder or
  tripwire hook whose host face is not sturdy; `W_UNSUPPORTED_BLOCK` a torch, lantern, hanging
  sign, pressure plate, redstone dust, repeater, comparator or door without the face it needs, or
  sand, gravel, concrete powder or an anvil over air or a fluid; `W_WATER_SPILL` water, or a block
  with `waterlogged=true`, that flows into air inside the structure. Cave and void air read as air,
  and Bedrock's flowing water and lava as water and lava. A body is not judged when an error
  refuses one of its members. `TargetRegistry` gains `constraint`, `holds` and `keeps_water`,
  which a registry with no catalog or no pinned version leaves at `None`, so it runs no check, and
  `contents`, the `Contents` a block reads as: air, water, lava or another block.
- *(lsp)* A document whose edition and version are both pinned raises the constraint catalog's
  checks, as the compile does.
- *(cli)* `cairn info`'s `semantic-sensitive` row names, per edition, each member that builds a
  block the catalog lists, with the reason and the boundary version, by its `id=` or, where it
  carries none, by its address (`raw_block[0]`). A lockfile records the catalog's hash as
  `constraint_catalog_hash` and the target's sensitive members as `member_version_sensitivity`.
