- *(eval)* The evaluation harness the specification's Evaluation Framework calls for, run as
  `cargo eval run`. It asks a model each of 50 prompts with only the English spec pages as its
  system prompt, checks each reply with this checkout's `cairn check` and `cairn compile`, returns
  what they print for up to 3 corrections, and writes a JSON run record holding the zero-shot
  compile rate, fix convergence rate, token efficiency and edit stability. Generation prompts cover
  walls, openings, roofs, levels, members, themes, `def` and `repeat`, redstone, columns and arches,
  and interiors, and edit prompts restart from the program their base converged on. A run names its
  model, edition and target, asks the Anthropic Messages API with `ANTHROPIC_API_KEY`, or answers
  from recorded replies with `--replay`. `cargo eval compare` sets a baseline record beside a
  proposal record, as the Operating rule asks a proposal to cite them.
- *(cli)* `cairn lower --edition <edition> --target <version>` lowers against that version's id
  table, as `cairn check` does with the same pair, and prints the IR `cairn compile` writes at that
  target. An unshipped version is refused before the file is read, and `--target` without
  `--edition` is a usage error.
