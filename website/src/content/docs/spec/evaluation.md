---
title: "13. Evaluation Framework"
---

The quality of the spec is iterated quantitatively, with a headless geometry simulator independent
of Minecraft itself, rather than by taste. Vocabulary and syntax debates drift into preference, so
the evaluation bench is fixed first.

```text
test prompt set (~50)
  → zero-shot generation
  → headless lint (syntax + AABB geometry: "window outside wall", "door hanging", …)
  → return line-numbered errors and self-correct for up to 3 turns
```

## 13.1 Primary metrics

| Metric | What it measures |
|---|---|
| **Zero-shot compile rate** | The fraction that compiles error-free on the first try. Measures how intuitive the spec is. |
| **Fix convergence rate** | The fraction that converges to error-free within 3 turns. Measures how expressive the error messages are. |
| **Token efficiency** | Expanded block count ÷ DSL token count. |
| **Edit stability** | How small the AST and NBT diffs are after a follow-up edit such as "make only the second-floor windows arched". |

## 13.2 Reverse-conversion auxiliary metrics

Reverse-conversion quality ([Ecosystem Interop](ecosystem-interop)) is removed from the primary
evaluation and treated as auxiliary, consistent with the lossy approach. What it measures is not
"did it reproduce the shape" but **"did it become editable DSL"**.

- `block_iou`, `state_accuracy` (facing / shape / waterlogged match), `entity_accuracy` (frame,
  sign, villager, display retention).
- `residual_ratio`: raw volume left after the lift. `compression_ratio`: voxel count ÷ token count.
- `editability_score`: named-member count, slot-ization rate, stable-address rate.
- `theme_extraction_score`: whether concrete blocks were separated into slot and theme rather than
  inlined.
- `symmetry_score`: the fraction folded into `repeat` / `mirror` / `def`. `version_portability`:
  canonical-token rate.

## 13.3 Operating rule

Vocabulary additions and syntax changes are adopted only in the direction that improves these
metrics, especially fix convergence rate and edit stability. One experiment settles most syntax and
vocabulary questions on real data: give the model only the spec, have it generate, and see where
the errors are.

The reverse-conversion harness doubles as an engine that grows the `def` and `theme` standard
library from a community schematic corpus:

```text
corpus → import → normalize(edition/version) → L1 compact → cluster(shape/material)
  → LLM lift candidates → compile/diff → human review → def/theme library
```

## 13.4 Redstone verification

The headless geometry simulator extends to a per-tick redstone logic simulator. It simulates the
synthesized circuit per target edition and checks it against the declared truth table and temporal
assertions, in a synth → sim → diff → patch loop. See [Redstone](redstone).
