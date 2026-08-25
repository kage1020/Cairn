---
title: "12. Ecosystem Interop and Reverse Conversion"
---

## 12.1 Forward direction

Serializing the block-array IR emits `.nbt`, `.litematic`, `.schem`, and `.mcstructure`
([Architecture](architecture)). Each format is just a serializer — existing formats are additional
backends around the pivot.

## 12.2 Reverse direction: the compiler transliterates, an LLM lifts

The compiler does not build voxel-to-"this is a wall" computer vision; that becomes unmaintainable.
It implements a robust faithful transliteration, verification, and voxel-diff. The meaning lift is
an LLM refactor of the raw-centric DSL — dogfooding the language, and consistent with P5's
self-correction loop.

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block centric
(an LLM refactors house.raw.crn into a semantic DSL) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn             # voxel XOR → into self-correction
```

The compile → diff → patch loop reports like this:

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

Convergence thresholds are block IoU ≥ 0.985, `state_accuracy` ≥ 0.995, and residual raw ≤ 5%. Exact
match is not required; the residue stays explicit as
`raw_fill id=residual_* origin=imported`.

## 12.3 Three tiers of transliteration

Naming is the boundary between transliteration and lift.

| Tier | What it is | Ceiling |
|---|---|---|
| **L0 — raw cells** | One voxel per line. Too large for LLM context, so it is an intermediate only. | — |
| **L1 — spatially compressed** | Fill aggregation, AABB palette compression, `resolved_state` → `intent_state` inversion (`stair facing=east half=top`), symmetry and period folded into `raw_repeat`. **No naming.** | The compiler's ceiling. |
| **L2 — semantically lifted** | fill → `wall`, repeat → `def` / `use`, concrete block → `mat_slot` + `theme`. | The LLM's ceiling. |

```
# L1 — no naming, deterministic
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=@glass_pane from=0,2,0 to=1,3,0
# L2 — the LLM names it and gives it meaning
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 Import stamping and pitfalls

On import, the `(edition, version)` pair and provenance are stamped onto the block-array IR —
`.litematic` → java, `.mcstructure` → bedrock, `.schem` → java. This is what connects import to
reproducibility ([Versioning and Editions](versioning-editions)).

**Never present import as "recovering author intent."** That is the biggest pitfall. Only voxels
and some regularity can be recovered, and the CLI says so with `W_SEMANTIC_LOSS`.

Other rules:

- Import-origin `raw_fill` is isolated with `origin=imported` / `residual` and is not treated as
  first-class design DSL.
- Litematica's multiple regions and sub-region offsets are preserved as provenance rather than
  flattened, and regions map to a `site` or several structs.
- For entity-bearing schematics, do not mark success on block IoU alone. Keep a separate entity
  metric and extract only first-class entities ([Entities](entities)) — chest contents and command
  blocks are dropped.
- Huge schematics (over 48³, or whole villages) blow up LLM context if lifted at once. They need an
  orchestration of chunk split → per-chunk L1 → per-part lift → join with `site`, over a streaming
  parse.
- Legacy numeric-ID `.schematic` files from before 1.13 flattening are not supported in v1
  ([Purpose and Scope](overview)).
