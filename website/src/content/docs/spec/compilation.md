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
stair slopes meeting at a ridge. The voxelisation rules are deliberately
minimal so the cottage example renders cleanly without committing to the
broader roof taxonomy (`shed`, `hip`, `flat`, ...) before evaluation data
arrives.

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
  (M2; per-theme roof materials land with the registry pack) with
  `half=bottom, shape=straight`, `facing` pointed toward the ridge:
  `south` on the `-z` slope and `north` on the `+z` slope for an x-axis
  ridge; `east` / `west` mirrored for a z-axis ridge. The apex caps with a
  single stair at `half=top` using the low-slope facing.
