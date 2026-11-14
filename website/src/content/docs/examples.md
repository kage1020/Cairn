---
title: "Examples"
description: Worked .crn files, each kept small so the language surface is the only thing on screen.
---

Every file lives in
[`examples/`](https://github.com/kage1020/Cairn/tree/main/examples) and is intentionally minimal.
The [Tutorial](/tutorial/) walks through the four in the first group.

> Every file here compiles with the current compiler, and a file with `logic` gets its circuit built
> under its floor.

## Start here

| File | Shows |
|---|---|
| [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) | The minimum useful build: `struct`, `theme`, slots. |
| [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) | Abstract material tokens, per-floor `level`, override-promotion. |
| [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn) | Logical redstone: signal binding, `circuit` region, assertions. |
| [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn) | Multi-building with `site` and topological `connect`. |

## Roof kinds

| File | Shows |
|---|---|
| [`roof-shed.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-shed.crn) | `kind=shed slope_to=front`: a single slope rising toward the front wall. |
| [`roof-hip.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-hip.crn) | `kind=hip` on a square footprint: four slopes onto one apex cap. |
| [`roof-flat.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-flat.crn) | `kind=flat`: a single deck layer, with the overhang extending it past the walls. |

## Walkways and ports

| File | Shows |
|---|---|
| [`l-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/l-walkway.crn) | A Manhattan L between two ports that differ on both axes. |
| [`at-side-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/at-side-walkway.crn) | `at=left` / `at=right` door anchors pulling ports toward the facing corners. |
| [`window-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/window-walkway.crn) | A window port, anchored at the rectangle's centre and pinned to the ground row. |

## Blockstates

| File | Shows |
|---|---|
| [`storeroom.crn`](https://github.com/kage1020/Cairn/blob/main/examples/storeroom.crn) | `chest` against the walls: each faces away from its wall, two side by side pair, and `size=single` keeps two apart. |
| [`workshop.crn`](https://github.com/kage1020/Cairn/blob/main/examples/workshop.crn) | `beam`, `trapdoor`, `note_block`, `bed` and `sign`: logs laid over a door and a window and along the side walls' tops, a trapdoor hung open against a wall and one laid high at a coordinate triple, a note block with the note and instrument its line writes, a brown bed with its head to the back wall, and a sign hung beside the door with the text its line writes. |
| [`control-room.crn`](https://github.com/kage1020/Cairn/blob/main/examples/control-room.crn) | Every sensor and actuator against the walls: a lever and a button hung facing away from theirs, a daylight detector, an observer and a sticky piston turned upward, a lamp, a dispenser facing out, and the piston bound through a patch. |
| [`aquarium.crn`](https://github.com/kage1020/Cairn/blob/main/examples/aquarium.crn) | `fill`: a sand bed painted with a material, then water poured around what stands in the tank, waterlogging a pane divider and a chest while `waterlogged=false` keeps another dry. |

## Entities

| File | Shows |
|---|---|
| [`trading-post.crn`](https://github.com/kage1020/Cairn/blob/main/examples/trading-post.crn) | A librarian villager by the back wall with the two trades it sells, a painting and an item frame hung on the side walls, an armor stand at a coordinate triple, and a `hologram` over the door that each edition's theme builds its own way: a text display on Java and a glowing sign on Bedrock. |

## Edge cases

| File | Shows |
|---|---|
| [`edition-fallback.crn`](https://github.com/kage1020/Cairn/blob/main/examples/edition-fallback.crn) | Per-edition theme variants for a slot with no cross-edition primitive, and `@edition` guards around a block each edition spells differently. |
| [`crossbar.crn`](https://github.com/kage1020/Cairn/blob/main/examples/crossbar.crn) | Two plates feed both gates, whose outputs drive doors at opposite ends, so two wires must cross: one runs over the other a layer up inside `void=4`, and at `void=3` the router refuses with `E_ROUTE_CONGESTION`. |
