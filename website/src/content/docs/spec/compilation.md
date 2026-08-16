---
title: "4. Compilation Model"
---

## 4.1 Phase evaluation
Source MAY be written line-oriented, flat, and order-free. The compiler assigns each command to an
implicit phase and evaluates them in a **fixed order**:

```
massing (shell: floor/walls/volume)
  → envelope (roof/exterior)
  → openings (door/window)
  → fixtures (furnishings: sign/painting/frame/bed/sensors & actuators)
  → logic_synth (redstone synthesis: Logic IR → Netlist IR)
  → logic_place (cell placement)
  → logic_route (routing → Placement IR, delay determined)
  → raw (escape hatch)
```

- A `window` written after `roof` in the source is still applied as an opening in the wall (order
  accidents are eliminated).
- **Last-wins applies only to local overrides within the same phase.** `raw` (fill, etc.) is the
  danger zone and is always applied last.
- Redstone logic ([Redstone](redstone)) splits the step right after `fixtures` into three
  phases: only once sensors/actuators are placed in 3D do their I/O port coordinates become fixed,
  enabling placement and routing.

```
struct keep size=11x9
floor  id=base   mat_slot=floor
walls  id=shell  mat_slot=wall height=5
roof   id=roof   kind=gable mat_slot=roof overhang=1
window id=front_windows side=front y=2 offset=2 size=2x2 mat_slot=glass   # opening cut even though after roof
door   id=entry  side=front at=center
```

## 4.2 Target axes
The target is the **two axes `(edition, version)`**. The version and edition are **not written in the
DSL source**. The only layer that knows the version/edition is the backend ([Versioning and Editions](versioning-editions)).

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

- `--target` alone is **forbidden**; `--edition` is **required**.
- The same "1.21" means different things on Java and Bedrock, and Java's DataVersion is unrelated to
  Bedrock's block_version.

## 4.3 Gable roof voxel rules

`roof kind=gable [overhang=N] mat_slot=...` lowers to a pair of opposite
stair slopes meeting at a ridge. The four supported roof kinds —
`gable`, `shed`, `hip`, and `flat` — share an overhang and wall-top
convention; the kind-specific layout rules are in §4.3–§4.6 below.

A sloped roof's material comes from its `mat_slot=` binding and **must be a
member of the stair family** — an id whose path ends in `_stairs`. The
geometry derives `facing`, `half`, and `shape` and attaches them to whatever
it paints, and a whole block has nowhere to put them: the result would be a
blockstate no version of the game has. The registry pack's four roof species
(`roof.dark_wood`, `roof.light_wood`, `roof.warm_wood`, `roof.cool_wood`) all
resolve inside the family, and choosing between them is what the binding is
for. A binding *outside* the family is `E_INCOMPATIBLE_MATERIAL` and stops
the build: attaching the states anyway writes a blockstate that does not
exist, and quietly substituting `minecraft:spruce_stairs` builds the roof
out of a material nobody chose — both are the silent substitution §10.4
forbids. With no `mat_slot=` at all that fallback does apply, silently,
because nothing was asked for.

A binding inside the family that also carries blockstates of its own keeps
its id and loses those states to the geometry, with `W_DEFERRED_MEMBER`.

The same rule governs an eave `stair kind=stairs` member: it takes its
states from its own arguments rather than from a slope, but it attaches them
to its material the same way.

- **Ridge axis.** The ridge runs along the *long* horizontal axis of the
  struct footprint. When the footprint is square (`size=WxW`) the tie
  breaks in favour of `x` (east-west ridge).
- **Ridge height.** A gable rises `ceil(short_span / 2)` voxels above the
  wall top, where `short_span` is the *roof bounding box* extent along the
  short axis (= `min(dims.x, dims.z)` after the overhang inflation below).
  The top layer is the apex: odd-span apexes cap with a single `half=top`
  stair on the centre row, even-span apexes cap with two `half=top` stairs
  on the adjacent meeting rows so the ridge does not leave an open V.
- **Overhang.** `overhang=N` inflates the voxel grid by `N` on every
  horizontal axis (`Dims.x = size.w + 2N`, `Dims.z = size.h + 2N`). Floors,
  walls, doors, and windows keep their authored coordinates and are
  shifted inward by `+N` along x and z. The roof spans the full inflated
  bounding box so the eaves and gable ends extend past the wall ring.
- **Stair orientation.** Each slope row sets `half=bottom,
  shape=straight` with `facing` pointed toward the ridge:
  `south` on the `-z` slope and `north` on the `+z` slope for an x-axis
  ridge; `east` / `west` mirrored for a z-axis ridge. The apex caps with a
  single stair at `half=top` using the low-slope facing.

## 4.4 Shed roof voxel rules

`roof kind=shed slope_to=front|back|left|right [overhang=N] mat_slot=...`
lowers to a single stair slope rising toward the wall named in
`slope_to=`. Each row is shaped like a gable's low slope —
`half=bottom, shape=straight` — but only one of the two slopes is emitted,
so the opposite wall stays at its authored height (no gable-end fill).

- **Slope axis.** When `slope_to=front|back` the slope rises along `z`;
  when `slope_to=left|right` it rises along `x`. The high edge sits on
  the wall named in `slope_to`; the low edge sits on the opposite wall.
- **Height.** A shed roof rises `slope_span` voxels above the wall top,
  where `slope_span` is the roof bounding-box extent along the slope
  axis (= `dims.z` for `slope_to=front|back`, `dims.x` for `slope_to=
  left|right`, after the overhang inflation). Each layer steps inward
  by 1 voxel from the low edge toward the high edge as `y` rises.
- **Stair orientation.** Every slope stair points toward the high edge:
  `slope_to=front` → `facing=south`, `back` → `north`, `left` → `west`,
  `right` → `east`. The top layer is the apex, capped with one row at
  `half=top` and the same facing so the peak closes.
- **Required argument.** `slope_to=` has no default — a `shed` without
  it surfaces `W_DEFERRED_MEMBER` rather than guessing a direction. An
  unknown `slope_to=` value reuses the same warning.

## 4.5 Hip roof voxel rules

`roof kind=hip [overhang=N] mat_slot=...` lowers to a four-sided stair
pyramid: all four walls slope inward toward a centre ridge.

- **Ridge axis and height.** Same as a gable — the ridge runs along the
  long axis (square footprint ties to `x`) and rises
  `ceil(short_span / 2)` voxels above the wall top.
- **Layer layout.** Each layer `L ∈ 0..extra_height` is the inset
  rectangle frame `[L, dims.x − 1 − L] × [L, dims.z − 1 − L]`:
  - north row (`z = L`): `facing=south, shape=straight`
  - south row (`z = dims.z − 1 − L`): `facing=north, shape=straight`
  - west column (`x = L`): `facing=east, shape=straight`
  - east column (`x = dims.x − 1 − L`): `facing=west, shape=straight`
  - the four corners use `shape=outer_*` so the diagonal closes:
    NW = `facing=south, outer_left`; NE = `facing=south, outer_right`;
    SW = `facing=north, outer_right`; SE = `facing=north, outer_left`.
- **Apex.** On a square footprint the apex is a single `half=top` stair
  (odd short span) or a `2x2` block of `half=top` stairs (even short
  span). On a rectangular footprint the apex collapses to a ridge row
  along the long axis: `roof_w == roof_h` length cap, otherwise a row
  of `half=top` stairs spanning the inset interior on the long axis.
  Apex facings follow the gable rule (`south` for an x-ridge, `east`
  for a z-ridge).
- **Overhang.** Same as `gable` — inflates the voxel grid by `N` on
  each horizontal axis; the roof covers the full inflated box.

## 4.6 Flat roof voxel rules

`roof kind=flat [overhang=N] mat_slot=...` lowers to a single layer of
solid blocks at `y = wall_top + 1`. The deck spans the entire inflated
bounding box (= `dims.x × dims.z`), so an `overhang=N` extends the deck
past the walls without any extra rules.

- **Material.** Every cell of the deck is the `mat_slot=` binding's id,
  attached to no blockstate, falling back to `minecraft:spruce_planks` when
  there is no binding. Unlike a sloped roof this constrains nothing: a deck
  is whole blocks, so every id is as valid as any other and a stair among
  them is simply a stair in its default state.
- **Height contribution.** A flat roof adds `1` to `Dims.y` regardless
  of footprint, so a `size=WxH` `walls height=K` plus `roof kind=flat`
  produces `Dims.y = 1 + K + 1`.
- **No slope arguments.** `slope_to=`, `kind=`-specific facings, and
  ridge axes do not apply.

## 4.7 Level grouping and volume derivation

`level y=N` groups members and places each of them `N` voxels above the
struct's base plane. It is a grouping construct rather than a member of
its own: the `level` line lowers to no blocks, and every member under it
lowers as if it had been written in the body with `N` added to whatever
vertical coordinate it already carries.

The volume a struct lowers into is derived, never written:

```
Dims.x = size.W + 2 × overhang
Dims.z = size.H + 2 × overhang
Dims.y = 1 + wall_top + roof_extra
```

`overhang` is the largest `overhang=` on any roof, `wall_top` the largest
`N + height` over the walls (`N` being the enclosing level's, `0` in the
body), and `roof_extra` the tallest per-kind contribution from §4.3–§4.6.
Members inside a `level` count in all three: a struct whose only walls sit
under `level y=5` is as tall as one that writes them directly.

Not every role has a lowering at a non-zero offset. `walls`, `door`,
`window`, `stair`, and `pressure_plate` read `N` as the base their own
geometry is measured from. A `floor` and a `roof` are single planes a
struct has one of — there is no second slab to drop and no second cap to
place — so under `level y=N` with `N > 0` each fires `W_DEFERRED_MEMBER`
and lowers to nothing.

A member dropped by the rule above contributes nothing to the derived
volume: the `overhang=` of a level-scoped roof does not widen the
footprint, and its height does not raise `Dims.y`. The converse holds
too — every member the pass paints is one the volume was sized to hold.
Those are two readings of a single list, which is what keeps a member
from painting past the end of the array it was handed.

The rule is about `level` grouping and does not generalise to every
member that lowers to nothing. A `roof` with no `kind=` still widens the
footprint by its `overhang=`, and `walls` whose material does not resolve
still raise `Dims.y`; both fire `W_DEFERRED_MEMBER` and paint no voxels.
The volume is derived before those failures are known, so today the extra
extent is air.
