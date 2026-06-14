---
title: "12. Ecosystem Interop and Reverse Conversion"
---

## 12.1 Forward direction
`block-array IR → serialize` emits `.nbt` / `.litematic` / `.schem` / `.mcstructure`
([Architecture](architecture)). Each format is just a serializer; existing formats are additional
backends around the block-array IR.

## 12.2 Reverse direction: the compiler only transliterates; an LLM does the lifting
The compiler does not build voxel→"this is a wall/roof" computer vision (it becomes unmaintainable).
**The compiler implements a robust faithful transliteration + verification + voxel-diff; the meaning
lift is done as an LLM refactor of the raw-centric DSL** (dogfooding the language). This is consistent
with P5's self-correction loop and the evaluation framework ([Evaluation Framework](evaluation)).

```text
cairn import house.litematic --mode raw    → house.raw.crn     # fill/raw_block centric (faithful transliteration)
(an LLM refactors house.raw.crn into a semantic DSL) → house.lifted.crn
cairn compile house.lifted.crn --edition java --target 1.21.4
cairn diff-blocks house.litematic house.lifted.crn           # voxel XOR → into self-correction
```

The compile→diff→patch self-correction loop:

```text
E_DECOMPILE_DIFF: block IoU = 0.962 < threshold 0.985
  missing bbox=(12,4,3)..(18,6,3) mat=glass_pane → likely window repeat too small
  Suggested patch: edit window[id=front_windows] set repeat=4
```

Convergence thresholds: block IoU ≥ 0.985 / state_accuracy ≥ 0.995 / residual raw ≤ 5%. Exact match is
not required; the residue is kept explicitly as `raw_fill id=residual_* origin=imported`.

## 12.3 Three tiers of faithful transliteration
"Naming" is the boundary between transliteration and lift.

- **L0 raw cells**: one voxel per line. Too large; it bloats LLM context, so it is an intermediate only.
- **L1 spatial-compressed (the compiler's ceiling)**: fill aggregation, AABB palette compression,
  **resolved_state → intent_state inversion** (`stair facing=east half=top`), symmetry/period as
  structural compression into `raw_repeat`. **But no naming.**
- **L2 semantic-lifted (the LLM's ceiling)**: fill→`wall`, repeat→`def/use`, concrete block→`mat_slot`+`theme`.

```
# L1 (no naming, deterministic)
raw_repeat id=r03 count=5 step=3,0,0: raw_fill mat=@glass_pane from=0,2,0 to=1,3,0
# L2 (the LLM names and gives meaning)
window id=front_windows side=front mat_slot=glass repeat=5 ...
```

## 12.4 Import stamping and pitfalls
- On import, stamp `(edition, version)` and provenance onto the block-array IR (`.litematic`→java,
  `.mcstructure`→bedrock, `.schem`→java). This connects to reproducibility/version awareness
  ([Versioning and Editions](versioning-editions)).
- **Do not present import as "recovering author intent"** (the biggest pitfall). Only voxels and some
  regularity can be recovered. Make this explicit in CLI/UI: `W_SEMANTIC_LOSS`.
- Import-origin `raw_fill` is isolated with `origin=imported` / `residual`; it is not treated as
  first-class design DSL.
- Preserve Litematica's multiple regions / sub-region offsets as provenance rather than flattening, and
  map regions to a `site` / multiple structs.
- For entity-bearing schematics, do not mark success on block IoU alone; keep a separate entity metric,
  and extract only first-class entities ([Entities](entities)) — drop chest contents/command
  blocks.
- Huge schematics (over 48³ / whole villages) blow up LLM context if lifted at once. Require an
  orchestration of **chunk split → per-chunk L1 → per-part lift → join with `site`** (streaming parse).
- Legacy numeric-ID `.schematic` (pre-1.13 flattening) is not supported in v1 ([Purpose and Scope](overview)).
