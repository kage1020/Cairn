---
title: "3. Architecture"
---

```
Intent DSL
   ↓ parse
Semantic / Component-Theme IR    named members: id / class / role / mat_slot / intent_state
   ↓ resolve                     phase evaluation, geometry expansion, theme injection, derived blockstate
block-array IR                   voxel grid + palette + block entities + entities   ← universal pivot
   ↓ serialize                   per edition, version, format backend
.nbt (Java) / .litematic / .schem / .mcstructure (Bedrock)
```

## 3.1 The block-array IR is the universal pivot

Every format's frontend and backend meets at this layer, and diff, IoU, and serialization all happen
here. It holds a voxel grid, a palette, block entities, and entities, and is neutral to format,
edition, and version.

It is the output target of the forward direction and the input destination of the reverse one
([Ecosystem Interop](ecosystem-interop)).

## 3.2 The Intent IR is rich and carries invariants

A named member carries `id`, `class`, `role`, `mat_slot`, `intent_state`, and `resolved_state`
([Blockstate Model](blockstate)).

A raw import does not produce a valid Intent IR. It reaches one only after a semantic lift, and an
artifact's progress along that path is `semantic_level: raw | grouped | lifted`.

## 3.3 Redstone sub-layers

When redstone is described logically ([Redstone](redstone)), three IR layers with distinct roles sit
between the Intent IR and the block-array IR, the same separation HDL uses:

```
Logic IR      logical expressions, dependency DAG. Edition-neutral, zero delay
Netlist IR    cells and nets. Logical cell selection. Still no delay
Placement IR  cell coordinates + actual wire length. Delay and ticks determined here
```

The logic is edition-neutral; the place-and-route result (tiles, timing) is edition-specific.
**Delay is not carried in the Logic or Netlist IR.** The Placement IR determines it.

## 3.4 What the split buys

The block-array IR at the bottom is shared across forward and reverse directions and across every
format. The member/Intent IR above it is an independent type with invariants: every member has an
`intent_state`, every slot is resolved, and so on.

That separation lets serialization, diff, lint, and IoU evaluation be shared at the bottom layer
while the semantic layer stays type-safe.
