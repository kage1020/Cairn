---
title: "4. Compilation Model"
---

## 4.1 Phase evaluation

Write commands flat and in any order. The compiler assigns each one to a phase and evaluates the
phases in this fixed order:

```
massing     floor / walls / volume
envelope    roof / stair
openings    door / window
fixtures    sign / painting / frame / bed / sensors / actuators
logic_synth redstone: Logic IR → Netlist IR
logic_place cell placement
logic_route routing → Placement IR, delay determined
raw         escape hatch
```

A `window` written after `roof` is still cut as an opening in the wall. Source order never decides
what a member means.

`circuit` writes no voxel — it only marks a routing region — so it belongs to no phase. The three
`logic_*` phases follow `fixtures` because port coordinates are not fixed until sensors and
actuators are placed in 3D. See [Redstone](redstone).

Last-wins applies only to **local overrides within the same phase**, and `raw` always runs last. Two
different members contesting a voxel inside a phase resolve the same way and are reported — see
§4.8.

```
struct keep size=11x9
floor  id=base   mat_slot=floor
walls  id=shell  mat_slot=wall height=5
roof   id=roof   kind=gable mat_slot=roof overhang=1
window id=front_windows side=front y=2 offset=2 size=2x2 mat_slot=glass   # still cut as an opening
door   id=entry  side=front at=center
```

## 4.2 Target axes

The target is the pair `(edition, version)`. Neither is written in the source; only the backend
knows them. See [Versioning and Editions](versioning-editions).

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

`--edition` is required and `--target` alone is refused. "1.21" means different things on Java and
Bedrock, and Java's DataVersion has nothing to do with Bedrock's block_version.

## 4.3 Gable roof voxel rules

`roof kind=gable [overhang=N] mat_slot=...` lowers to two opposite stair slopes meeting at a ridge.

The four roof kinds — `gable`, `shed`, `hip`, `flat` — share the overhang and wall-top conventions
below; their layouts are §4.3–§4.6.

**Material.** A sloped roof takes its material from `mat_slot=` and it MUST be in the stair family
— an id whose path ends in `_stairs`. The geometry attaches `facing`, `half`, and `shape` to
whatever it paints, and a whole block cannot carry them. A binding outside the family is
`E_INCOMPATIBLE_MATERIAL` and stops the build. With no `mat_slot=` at all the roof falls back to
`minecraft:spruce_stairs`. The registry pack's four roof species (`roof.dark_wood`,
`roof.light_wood`, `roof.warm_wood`, `roof.cool_wood`) all resolve inside the family.

A binding inside the family that carries blockstates of its own keeps its id and loses those states
to the geometry, with `W_DEFERRED_MEMBER`. An eave `stair kind=stairs` follows the same material
rule, but takes its states from its own arguments.

**Ridge axis.** The ridge runs along the long horizontal axis of the footprint. A square footprint
(`size=WxW`) ties to `x`, giving an east-west ridge.

**Ridge height.** A gable rises `ceil(short_span / 2)` voxels above the wall top, where
`short_span` is `min(dims.x, dims.z)` after the overhang inflation.

**Layers.** Layer `0` seats on the wall top and is a pair of slope rows — one row when `short_span`
is 1 and the two converge. Each layer above steps inward by one on each side. The topmost layer is
the apex:

- odd span: one `half=top` stair on the centre row.
- even span: two `half=top` stairs on the adjacent meeting rows, so the ridge leaves no open V.

A `short_span` of 1 or 2 rises exactly one layer, which is layer `0`, and has no apex course.

**Overhang.** `overhang=N` inflates the voxel grid by `N` on both horizontal axes
(`Dims.x = size.w + 2N`, `Dims.z = size.h + 2N`). Floors, walls, doors, and windows keep their
authored coordinates and shift inward by `+N`. The roof spans the full inflated box, so eaves and
gable ends extend past the wall ring.

**Stair orientation.** Slope rows are `half=bottom, shape=straight` with `facing` pointed at the
ridge: `south` on the `-z` slope and `north` on the `+z` slope for an x-axis ridge, `east` / `west`
mirrored for a z-axis ridge.

An even span's two apex stairs each face *away* from the ridge (`north` on the `-z` row, `south` on
the `+z` row for an x-axis ridge). Facing inward would leave a 0.5 × 0.5 undercut along both outer
faces for the roof's whole length; facing outward moves that void under the ridge.

An odd span's single apex stair is `half=top` with the low-slope facing. One cell has two outer
faces and a stair serves one, so the void is unavoidable and the rule just fixes the choice.

## 4.4 Shed roof voxel rules

`roof kind=shed slope_to=front|back|left|right [overhang=N] mat_slot=...` lowers to a single stair
slope rising toward the wall named in `slope_to=`. Rows are shaped like a gable's low slope
(`half=bottom, shape=straight`), but only one slope is emitted, so the opposite wall keeps its
authored height.

- **Slope axis.** `slope_to=front|back` rises along `z`; `slope_to=left|right` rises along `x`. The
  high edge sits on the named wall, the low edge on the opposite one.
- **Height.** A shed rises `slope_span` voxels above the wall top — `dims.z` for `front|back`,
  `dims.x` for `left|right`, after overhang inflation. Each layer steps inward by 1 from the low
  edge as `y` rises.
- **Stair orientation.** Every slope stair points at the high edge: `front` → `facing=south`,
  `back` → `north`, `left` → `west`, `right` → `east`. The top layer is capped with one row at
  `half=top` and the same facing.
- **`slope_to=` is required.** It has no default. A missing or unknown value is
  `W_DEFERRED_MEMBER` rather than a guessed direction.

## 4.5 Hip roof voxel rules

`roof kind=hip [overhang=N] mat_slot=...` lowers to a four-sided stair pyramid: all four walls
slope inward toward a centre ridge.

- **Ridge axis and height.** As `gable` — long axis, square ties to `x`, `ceil(short_span / 2)`
  above the wall top.
- **Layer layout.** Layer `L ∈ 0..extra_height` is the inset rectangle frame
  `[L, dims.x − 1 − L] × [L, dims.z − 1 − L]`. Layer `0` seats on the wall top and is always this
  frame, even when it is also the last layer:

  | Edge | States |
  |---|---|
  | north row (`z = L`) | `facing=south, shape=straight` |
  | south row (`z = dims.z − 1 − L`) | `facing=north, shape=straight` |
  | west column (`x = L`) | `facing=east, shape=straight` |
  | east column (`x = dims.x − 1 − L`) | `facing=west, shape=straight` |
  | NW / NE corners | `facing=south` with `outer_left` / `outer_right` |
  | SW / SE corners | `facing=north` with `outer_right` / `outer_left` |

- **Apex.** The apex closes what the frames below it raised, so it applies only when
  `extra_height > 1`. A square footprint caps with a single `half=top` stair (odd short span) or a
  `2x2` block of them (even short span). A rectangular footprint caps with a row of `half=top`
  stairs spanning the inset interior along the long axis. Apex facings follow the gable rule:
  `south` for an x-ridge, `east` for a z-ridge.
- **Overhang.** As `gable`.

## 4.6 Flat roof voxel rules

`roof kind=flat [overhang=N] mat_slot=...` lowers to one layer of solid blocks at
`y = wall_top + 1`, spanning the whole inflated bounding box.

- **Material.** Every deck cell is the `mat_slot=` binding's id with no blockstate, falling back to
  `minecraft:spruce_planks`. A deck is whole blocks, so unlike a sloped roof any id is valid — a
  stair among them is just a stair in its default state.
- **Height.** A flat roof adds `1` to `Dims.y` whatever the footprint, so `size=WxH` with
  `walls height=K` gives `Dims.y = 1 + K + 1`.
- **No slope arguments.** `slope_to=`, kind-specific facings, and ridge axes do not apply.

## 4.7 Level grouping and volume derivation

`level y=N` groups members and places each of them `N` voxels above the struct's base plane. The
`level` line itself lowers to no blocks; every member under it lowers as if written in the body with
`N` added to its vertical coordinate.

The volume a struct lowers into is derived, never written:

```
Dims.x = size.W + 2 × overhang
Dims.z = size.H + 2 × overhang
Dims.y = 1 + wall_top + roof_extra
```

Each term counts only the members that will actually paint:

- `overhang` — the largest `overhang=` on any roof that will draw (a `kind=` the compiler knows,
  plus a `slope_to=` if that kind is `shed`).
- `wall_top` — the largest `N + height` over the walls whose `mat_slot=` resolves, `N` being the
  enclosing level's offset and `0` in the body.
- `roof_extra` — the tallest per-kind contribution from §4.3–§4.6.
- `1` — the base plane, which every struct has.

Members inside a `level` count in all three: a struct whose only walls sit under `level y=5` is as
tall as one that writes them directly.

**Not every role lowers at a non-zero offset.** `walls`, `door`, `window`, `stair`, and
`pressure_plate` read `N` as the base their geometry is measured from. A `floor` and a `roof` are
planes a struct has one of, so under `level y=N` with `N > 0` each fires `W_DEFERRED_MEMBER` and
lowers to nothing.

**A member that does not paint does not size the volume.** The `overhang=` of a level-scoped roof
does not widen the footprint, and its height does not raise `Dims.y`. The same holds for every way
a member drops out: a `roof` with no `kind=` or a `shed` with no `slope_to=` does not widen the
footprint, and `walls` whose material does not resolve do not raise `Dims.y`.

The material half applies to `walls` and not to `roof`, because a roof whose `mat_slot=` does not
resolve falls back to a material of its own and still draws. A themeless struct shows the
asymmetry: its walls lower to air and reserve nothing, while a `roof kind=gable` over them still
draws and still seats its ridge above them.

"Does the material resolve" is asked against the pinned target, so a block only some versions
declare can change `Dims.y` between two `--target` values. An id the pinned target does not declare
is `E_UNKNOWN_ID`, so no artifact ships from that shape.

## 4.8 Within-phase conflicts and the palette

Across phases, the phase order decides: a `door` cut through `walls` is massing followed by
openings, and the hole is the point. Inside one phase, only source order separates two members,
which is what §4.1 grants to "local overrides within the same phase".

That grant is for an author restating a member. Two footprints that merely intersect are a
different thing, so the compiler keeps the last write and emits `W_PHASE_CONFLICT` naming both
members and how many voxels changed hands. The build is unchanged; the author is told that a line
they could move is deciding the result.

Two cases are not conflicts:

- A cell whose value does not change — two `walls` of one material meeting over shared rows.
- A member writing over itself — a `window` whose `repeat=` / `step=` stamps overlap.

**The palette** of an evaluated body — a `struct`, a `def`, and each `place` that instantiates one —
lists the blocks that body contains, in the order the phases first painted them, with air at slot
`0`. It is not a log of everything interned along the way: a material whose last voxel a later
phase covered is dropped, and the remaining slots renumber onto the gap. Otherwise two sources
differing only in which member lost would produce different artifacts for the same build, since the
loser would reach the `.nbt`, be counted by `cairn info`, and be covered by `resolved_ir_hash`.

A walkway's array is laid by the `connect` pass rather than by the phases, and is not covered here.
