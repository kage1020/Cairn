---
title: "8. Entities"
---

## 8.1 Two-tier model
Fully opening up `nbt={}` destroys theme/edit/lint/version-tolerance. **Important entities are
structured** so they ride the named-member editing model.

Promotion criterion: **an entity with "attributes you want to edit" or "attributes that absorb version
differences" is structured. Only one-off special NBT escapes through `nbt={}`.**

- **First-class, structured**: `sign`, `painting`, `item_frame`, `armor_stand`, `villager` (+`trade`),
  `display` (text/block/item), `bed` (treated as a block).
- **Generic fallback**: `spawn id=.. type=<entity> at=<selector> [nbt={...}]` (other mobs).

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

A villager trading hall is a staple build and display entities are core to modern decoration; sending
them to `nbt={}` drops generation quality and edit stability, so they are structured.

Block entities (signs, etc.) and true entities (paintings, etc.) are different things in NBT but share
one selector grammar in the DSL. The distinction is the compiler's responsibility.

## 8.2 Anchor conventions for variable-size elements (top open issue)
Paintings, item frames, arch windows, stairwells, and overhanging roofs have a declared size that
differs from the actually-occupied AABB. Left ambiguous, edit stability, theme swapping, and
cross-implementation compatibility all break.

- **Every primitive carries `anchor` (reference point), declared bbox, actual bbox, and host face in
  the IR.**
- Fix the resolution rule for overlapping AABBs in the spec: either **priority merge or a lint error**
  ([Lint](lint)).
- Neighbor-dependent blockstate (stairs, fences) breaks ("an inner-corner stair left as outer, hanging
  in mid-air") if overwritten without interference detection. Boundary blockstate re-resolution is the
  IR layer's responsibility.

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 Where entities are drawn
Signs, paintings, item frames, and beds contribute to architectural "feel" and are adopted. Chest
contents, villager inventory, and other information that does not contribute to architectural
precision is not structured and goes to the generic `spawn` `nbt={}` or the escape hatch.
