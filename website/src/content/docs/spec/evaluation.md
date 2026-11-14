---
title: "13. Evaluation Framework"
---

The spec improves by measurement, not taste, using a headless geometry simulator that does not
depend on Minecraft itself. Vocabulary and syntax debates drift into preference, so the evaluation
bench comes first.

```text
test prompt set (~50)
  → zero-shot generation
  → headless lint (syntax + AABB geometry: "window outside wall", "door hanging", …)
  → return line-numbered errors and self-correct for up to 3 turns
```

The repository's evaluation harness runs this loop. `cargo eval run` asks a model every prompt of
the set in `eval/prompts/`, checks each reply with this checkout's `cairn`, and writes a
[run record](#135-run-record) holding the metrics below; `cargo eval compare` sets two records side
by side. The [developer guide](/development/#the-evaluation-harness) covers running it.

The lint step is `cairn check` and `cairn compile` at the run's target, so the loop sees what an
author sees and the harness runs no geometry of its own. Of the two geometry faults above, a window
outside its wall is `E_INVALID_MEMBER`. A door hanging in mid-air is the part of the geometry
category no command checks yet ([Lint](/spec/lint/#semantic-categories)), so a program holding one
can pass. Once `cairn check` reports it, the loop sees it with no change to the harness.

## 13.1 Primary metrics

| Metric | What it measures |
|---|---|
| **Zero-shot compile rate** | The fraction that compiles error-free on the first try. Measures how intuitive the spec is. |
| **Fix convergence rate** | The fraction that converges to error-free within 3 turns. Measures how expressive the error messages are. |
| **Token efficiency** | Expanded block count ÷ DSL token count. |
| **Edit stability** | How small the AST and NBT diffs are after a follow-up edit such as "make only the second-floor windows arched". |

The metrics rest on these terms:

- **Prompt.** One file of the prompt set: a request in plain English under a topic. The topics are
  walls, openings, roofs, levels, members, themes, `def` and `repeat`, redstone, columns and
  arches, and interiors. A **generation prompt** asks for a build. An **edit prompt** names a
  generation prompt as its base and asks for a change to the program that prompt converged on.
- **Conversation.** The model is given a short preamble and every English page of this
  specification as its system prompt, and nothing else from the repository. The first message
  names the target through the `cairn compile` command line and carries the prompt; an edit
  prompt's first message also quotes its base's program. An edit prompt whose base did not
  converge is skipped.
- **Attempt.** One reply. Its program is the last fenced code block marked `cairn` or `crn`. An
  attempt **passes** when it holds a program, `cairn check` reports no error on it, `cairn compile`
  builds it, and the build holds at least one block. A warning does not fail an attempt. A failed
  attempt is answered with what `cairn check` or `cairn compile` printed, unchanged, or with a line
  saying the reply held no program or built no block.
- **Convergence.** A conversation **converges** when one of its first four attempts passes: the
  first, and up to 3 corrections. It ends at the attempt that passes.
- **Block.** A cell other than air in a structure `cairn compile` writes at the target, as
  `cairn lower --target` prints it: its structure, its position, and its block id, block states and
  block entity. An entity is not a block.
- **DSL token.** A token of [Lexical](/spec/syntax/#51-lexical), save the end of a line and the
  indents and dedents that nesting makes. A comment is not a token. `slot wall -> @cobblestone` is
  five tokens.
- **AST node.** A leading `@` directive, a top-level item's own line, or a line of a body, at its
  depth, with its arguments in key order and without the lines nested under it.

Over those terms, the metrics are:

- **Zero-shot compile rate** is the generation prompts whose first attempt passes ÷ the generation
  prompts.
- **Fix convergence rate** is the generation prompts that converge ÷ the generation prompts.
- **Token efficiency** is the blocks ÷ the DSL tokens of the converged programs, each summed over
  the generation prompts, so a large build weighs more than a small one.
- **Edit stability** is two ratios for each edit prompt that converged, each averaged over those
  prompts. Smaller is more stable.
  - **AST diff**: (nodes removed + nodes added) ÷ (nodes before + nodes after), where the nodes kept
    are a longest common subsequence of the two programs' nodes. Reordering arguments or changing
    comments and spacing is no change; moving a line under another is.
  - **NBT diff**: the cells whose block differs ÷ the cells either build fills, taken on the blocks
    `cairn compile` writes rather than read back from its files. A block state the compiler
    derives, such as a pane's connections, counts too
    ([Editing model](/spec/components-editing-sites/#92-editing-model)).

An edit prompt's outcome depends on its base's program, so the two rates count generation prompts
only, and the run record also breaks them down by topic.

## 13.2 Reverse-conversion auxiliary metrics

Reverse-conversion quality ([Ecosystem Interop](/spec/ecosystem-interop/)) is kept out of the
primary evaluation and treated as auxiliary, consistent with the lossy approach. What it measures is
not "did it reproduce the shape" but **"did it become editable DSL"**.

- `block_iou`, `state_accuracy` (facing / shape / waterlogged match), `entity_accuracy` (frame,
  sign, villager, display retention).
- `residual_ratio`: raw volume left after the lift. `compression_ratio`: voxel count ÷ token count.
- `editability_score`: named-member count, slot-ization rate, stable-address rate.
- `theme_extraction_score`: whether concrete blocks were separated into slot and theme rather than
  inlined.
- `symmetry_score`: the fraction folded into `repeat` / `mirror` / `def`. `version_portability`:
  canonical-token rate.

## 13.3 Operating rule

A vocabulary addition or syntax change is adopted only if it improves these metrics, especially fix
convergence rate and edit stability. One experiment settles most syntax and vocabulary questions on
real data: give the model only the spec, have it generate, and see where the errors are.

A proposal cites two runs: a **baseline run** at the commit it branches from, and a **proposal
run** at its head. Both ask the same model, prompt set, edition, target and `max_tokens`; a
proposal that changes the prompt set runs its baseline with the proposal's set. The proposal names
both records by their `spec_pages` and `prompt_set` hashes and their compiler versions, and quotes
what `cargo eval compare` prints for them. The comparison refuses two records that did not ask the
same thing, and takes token efficiency and edit stability over the prompts both runs converged on.
A model does not answer the same way twice, so one pair of runs settles only a wide margin; a
proposal whose margin is a few prompts cites more pairs.

The reverse-conversion harness doubles as an engine that grows the `def` and `theme` standard
library from a community schematic corpus:

```text
corpus → import → normalize(edition/version) → L1 compact → cluster(shape/material)
  → LLM lift candidates → compile/diff → human review → def/theme library
```

## 13.4 Redstone verification

The headless geometry simulator extends to a per-tick redstone logic simulator. It simulates the
synthesized circuit per target edition and checks it against the declared truth-table, latency and
temporal assertions, in a synth → sim → diff → patch loop. The redstone simulator exists: every
command that builds a circuit runs it against `assert truth` and `assert always`, and counts a
latency bound's ticks on the legalized circuit. A failed assertion is an error, which the
harness's loop returns to the model with the rest of the report; nothing patches the layout itself.
See [Redstone](/spec/redstone/#147-verification).

## 13.5 Run record

A run writes one JSON record. It names what the run asked: the record's `format`, the
`compiler_version` of the checkout, the `model` id, the `edition` and `target`, the `max_tokens`
each request carries, and two hashes. `prompt_set` is the sha256 of the prompts and the fixed
messages the harness sends, and `spec_pages` the sha256 of the spec pages the system prompt carries.
The record then holds the metrics of [§13.1](#131-primary-metrics), the requests the run made and
the tokens they used, and each prompt's outcome: every attempt's reply, verdict, diagnostics and
the answer it drew, and the program it converged on with that program's DSL tokens, blocks and, for
an edit prompt, its diffs.

The record names the model it asked. The harness has no default model, so `--model` is required
and every run names one.
