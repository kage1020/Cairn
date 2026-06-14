# 4. Compilation Model

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
- Redstone logic ([redstone.md](redstone.md)) splits the step right after `fixtures` into three
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
DSL source**. The only layer that knows the version/edition is the backend ([versioning-editions.md](versioning-editions.md)).

```sh
cairn compile build.crn --edition java    --target 1.21.4
cairn compile build.crn --edition bedrock --target 1.21.40
```

- `--target` alone is **forbidden**; `--edition` is **required**.
- The same "1.21" means different things on Java and Bedrock, and Java's DataVersion is unrelated to
  Bedrock's block_version.
