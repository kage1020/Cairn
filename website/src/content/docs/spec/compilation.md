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
convention; the kind-specific layout rules are in §4.3–§4.6 below. Per-theme
roof materials follow with the registry pack — until then every sloped
roof emits `minecraft:spruce_stairs` and every flat roof emits
`minecraft:spruce_planks`; a `mat_slot=` binding that resolves to any
other id fires `W_DEFERRED_MEMBER` so the user's intent is not silently
replaced.

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
- **Stair orientation.** Each slope row uses `minecraft:spruce_stairs`
  with `half=bottom, shape=straight`, `facing` pointed toward the ridge:
  `south` on the `-z` slope and `north` on the `+z` slope for an x-axis
  ridge; `east` / `west` mirrored for a z-axis ridge. The apex caps with a
  single stair at `half=top` using the low-slope facing.

## 4.4 Shed roof voxel rules

`roof kind=shed slope_to=front|back|left|right [overhang=N] mat_slot=...`
lowers to a single stair slope rising toward the wall named in
`slope_to=`. The slope is the same family as a gable's low slope —
`minecraft:spruce_stairs` with `half=bottom, shape=straight` — but only
one of the two slopes is emitted, so the opposite wall stays at its
authored height (no gable-end fill).

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

- **Material.** Every cell of the deck is `minecraft:spruce_planks`.
  Per-theme flat-roof materials are deferred to the registry pack
  catalogue; until then a `mat_slot=` binding that resolves to anything
  else fires `W_DEFERRED_MEMBER`, matching the sloped-roof contract.
- **Height contribution.** A flat roof adds `1` to `Dims.y` regardless
  of footprint, so a `size=WxH` `walls height=K` plus `roof kind=flat`
  produces `Dims.y = 1 + K + 1`.
- **No slope arguments.** `slope_to=`, `kind=`-specific facings, and
  ridge axes do not apply.
