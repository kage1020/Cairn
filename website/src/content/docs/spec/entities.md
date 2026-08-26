---
title: "8. Entities"
---

## 8.1 Two tiers

Opening up `nbt={}` for everything would destroy theming, editing, linting, and version tolerance.
So important entities are **structured**, which lets them ride the named-member editing model.

The promotion criterion: an entity with attributes you want to edit, or attributes that absorb
version differences, is structured. Only one-off special NBT escapes through `nbt={}`.

- **Structured, first-class**: `sign`, `painting`, `item_frame`, `armor_stand`, `villager` (plus
  `trade`), `display` (text / block / item), and `bed` (treated as a block).
- **Generic fallback**: `spawn id=.. type=<entity> at=<selector> [nbt={...}]` for other mobs.

```
villager id=trader at=stall[0] profession=librarian level=master
trade villager=trader buy=emerald count=24 sell=enchanted_book enchant=mending
text_display  id=holo   at=4,3,2 text="Inn" billboard=fixed scale=1.5
block_display id=model  at=front.above block=@lantern scale=0.5
item_display  id=trophy at=counter item=diamond_sword rotation=y90
spawn id=cat type=cat at=inside.floor nbt={variant:"black"}
```

A villager trading hall is a staple build and display entities are core to modern decoration.
Sending them to `nbt={}` would cost generation quality and edit stability, so they are structured.

Block entities (signs) and true entities (paintings) are different things in NBT but share one
selector grammar in the DSL. Telling them apart is the compiler's job.

## 8.2 Anchor conventions

Paintings, item frames, arch windows, stairwells, and overhanging roofs have a declared size that
differs from the AABB they occupy. Left ambiguous, edit stability, theme swapping, and
cross-implementation compatibility all break. This is the top open issue in this chapter.

Every primitive therefore carries four things in the IR: an `anchor` (reference point), a declared
bbox, an actual bbox, and a host face.

Overlapping AABBs resolve by a rule the spec fixes, either priority merge or a lint error
([Lint](lint)). Neighbour-dependent blockstate (stairs, fences) breaks when overwritten without
interference detection: an inner-corner stair left as an outer corner, hanging in mid-air.
Re-resolving boundary blockstate is the IR layer's responsibility.

```
painting id=hall_art side=inside.front anchor=center y=2 variant=kebab
window   id=arch1    side=front anchor=bottom_center offset=4 y=2 size=3x3 shape=arch
roof     id=roof     kind=gable footprint=struct overhang=1 bounds=expand
```

## 8.3 Where the line is drawn

Signs, paintings, item frames, and beds contribute to a build's architectural feel, so they are
adopted. Chest contents, villager inventory, and other information that does not contribute to
architectural precision is not structured. It goes to the generic `spawn` `nbt={}` or the escape
hatch.
