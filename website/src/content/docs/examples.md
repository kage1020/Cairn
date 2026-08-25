---
title: "Examples"
description: Worked .crn files, each kept small so the language surface is the only thing on screen.
---

Every file lives in
[`examples/`](https://github.com/kage1020/Cairn/tree/main/examples) and is intentionally minimal.
The [Tutorial](/tutorial/) walks through the four in the first group.

> The reference compiler is not finished, so these are normative illustrations rather than files you
> can build today.

## Start here

| File | Shows |
|---|---|
| [`cottage.crn`](https://github.com/kage1020/Cairn/blob/main/examples/cottage.crn) | The minimum useful build: `struct`, `theme`, slots, wall selectors. |
| [`themed-tower.crn`](https://github.com/kage1020/Cairn/blob/main/examples/themed-tower.crn) | Abstract material tokens, per-floor `level`, override-promotion. |
| [`redstone-door.crn`](https://github.com/kage1020/Cairn/blob/main/examples/redstone-door.crn) | Logical redstone: signal binding, `circuit` region, assertions. |
| [`village.crn`](https://github.com/kage1020/Cairn/blob/main/examples/village.crn) | Multi-building with `site` and topological `connect`. |

## Roof kinds

| File | Shows |
|---|---|
| [`roof-shed.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-shed.crn) | `kind=shed slope_to=front` — a single slope rising toward the front wall. |
| [`roof-hip.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-hip.crn) | `kind=hip` on a square footprint — four slopes onto one apex cap. |
| [`roof-flat.crn`](https://github.com/kage1020/Cairn/blob/main/examples/roof-flat.crn) | `kind=flat` — a single deck layer, with the overhang extending it past the walls. |

## Walkways and ports

| File | Shows |
|---|---|
| [`l-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/l-walkway.crn) | A Manhattan L between two ports that differ on both axes. |
| [`at-side-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/at-side-walkway.crn) | `at=left` / `at=right` door anchors pulling ports toward the facing corners. |
| [`window-walkway.crn`](https://github.com/kage1020/Cairn/blob/main/examples/window-walkway.crn) | A window port — anchored at the rectangle's centre and pinned to the ground row. |

## Edge cases

| File | Shows |
|---|---|
| [`edition-fallback.crn`](https://github.com/kage1020/Cairn/blob/main/examples/edition-fallback.crn) | Per-edition theme variants for a slot with no cross-edition primitive. |
| [`crossbar.crn`](https://github.com/kage1020/Cairn/blob/main/examples/crossbar.crn) | Two nets sharing a wire coordinate — what the crossing pass reports and does not repair. |
