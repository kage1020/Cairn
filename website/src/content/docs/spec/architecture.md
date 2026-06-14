---
title: "3. Architecture (three-layer IR + universal pivot)"
---

```
Intent DSL
   ↓ parse
Semantic / Component-Theme IR     … named members (id/class/role/mat_slot/intent_state)
   ↓ resolve (phase evaluation, geometry expansion, theme injection, derived blockstate)
block-array IR                    … voxel grid + palette + block entities + entities [universal pivot]
   ↓ serialize (per edition, version, format backend)
{ .nbt (Java) / .litematic / .schem / .mcstructure (Bedrock) }
```

## 3.1 block-array IR = universal pivot
- Every format's frontend/backend meets at this layer. **diff / IoU / serialization happen here.**
- It holds a voxel grid + palette + block entities + entities, neutral to format, edition, and version.
- It is the output target of the forward direction and the input destination of the reverse direction
  ([ecosystem-interop.md](ecosystem-interop)).

## 3.2 The member / Intent IR is rich and carries invariants
- A named member carries `id` / `class` / `role` / `mat_slot` / `intent_state` / `resolved_state`
  ([blockstate.md](blockstate)).
- A raw import (schematic ingestion) does not produce a valid Intent IR; it reaches one only after a
  semantic lift.
- An artifact's progress is expressed by `semantic_level: raw | grouped | lifted`.

## 3.3 Redstone logic sub-layers (Logic / Netlist / Placement IR)
When redstone is described logically ([redstone.md](redstone)), three IR layers with distinct roles
sit between the Intent IR and the block-array IR (a standard separation in HDL):
```
Logic IR     logical expressions / dependency DAG (edition-neutral, zero delay)
Netlist IR   cells/nets (logical cell selection; still carries no delay)
Placement IR cell coordinates + actual wire lengths (delay/tick first determined here)
```
The logic is edition-neutral; the place-and-route result (tiles, timing) is edition-specific. The key
point: **delay is not carried in Logic/Netlist; it is determined in the Placement IR.**

## 3.4 Consequence of the two-layer model
- **The bottom block-array IR is shared** (across forward/reverse and all formats).
- **The member/Intent IR above it is an independent type** with invariants (every member has an
  intent_state, slots are resolved, etc.).
- This separation lets serialization, diff, lint, and evaluation (IoU) be shared at the bottom layer
  while keeping the semantic layer type-safe.
