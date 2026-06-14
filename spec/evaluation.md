# 13. Evaluation Framework

The quality of the spec is iterated quantitatively with a **headless geometry simulator** (independent
of Minecraft itself), not by taste. Vocabulary/syntax debates drift into preference, so the evaluation
bench is fixed first.

```text
test prompt set (~50)
  → zero-shot generation
  → headless lint (syntax + AABB geometry expansion detecting "window outside wall", "door hanging", etc.)
  → return line-numbered errors and self-correct for up to 3 turns
```

## 13.1 Primary metrics
- **Zero-shot Compile Rate**: fraction that compiles error-free on the first try (= how intuitive the
  spec is).
- **Fix Convergence Rate**: fraction that converges to error-free within 3 turns (= the expressiveness
  of error messages).
- **Token Efficiency**: expanded block count / DSL token count.
- **Edit Stability**: how small the AST diff / NBT diff is after a follow-up edit such as "make only the
  second-floor windows arched".

## 13.2 Reverse-conversion auxiliary metric (decompile_quality)
The quality of reverse conversion ([ecosystem-interop.md](ecosystem-interop.md)) is removed from the
primary evaluation and treated as an auxiliary metric (consistent with the lossy approach). The core is
to measure not "did it reproduce the shape" but **"did it become editable DSL"**.

- `block_iou`, `state_accuracy` (facing/shape/waterlogged match), `entity_accuracy`
  (frame/sign/villager/display retention)
- `residual_ratio` (raw volume left after lift), `compression_ratio` (voxel count / token count)
- `editability_score` (named-member count, slot-ization rate, stable-address rate)
- `theme_extraction_score` (whether concrete blocks were separated into slot/theme instead of inlined)
- `symmetry_score` (fraction folded into repeat/mirror/def), `version_portability` (canonical-token rate)

## 13.3 Operating rule
Adopt vocabulary additions / syntax changes only in the direction that improves these metrics
(especially Fix Convergence Rate and Edit Stability). Running the "give the model only the spec, have it
generate, and observe where the errors are" experiment settles most syntax/vocabulary questions on real
data.

The reverse-conversion evaluation harness doubles as an engine that grows the `def` / `theme` standard
library from a community schematic corpus:

```text
corpus → import → normalize(edition/version) → L1 compact → cluster(shape/material)
  → LLM lift candidates → compile/diff → human review → def/theme library
```

## 13.4 Redstone verification
The headless geometry sim extends to a **per-tick redstone logic simulator**. It simulates the
synthesized circuit per target edition and checks it against the declared truth table / temporal
assertions (synth→sim→diff→patch). See [redstone.md](redstone.md).
