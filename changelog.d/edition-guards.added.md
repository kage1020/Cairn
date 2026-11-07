- *(core)* `@edition java` and `@edition bedrock` guards, on a line of their own in a `struct`,
  `def` or `site` body or under a member there, with the lines they guard indented under them. A
  guard holds the escape hatch, `raw_block` and `raw_fill`: any other member, a `logic` line or an
  `assert` line under one is the new `E_SEMANTIC_IN_EDITION_GUARD`, since the semantic layer is
  edition-neutral. A guard opens no scope, so its lines are checked where it stands. A build that
  pins an edition keeps its own guards' lines and drops the other's, and checks the ids it keeps
  against the pinned version. Lines under guards for different editions may share an `id=`. A
  `cairn check` that pins no edition checks each edition's branch, and reports a finding on a
  guarded line as that edition's.
- *(cli)* `cairn lower`, which pins no edition, leaves out every member under a guard, and a note on
  stderr says how many it left out. `cairn info` counts each edition's branch in that edition's
  rows.
- *(lsp)* Completion offers `edition` after a line-opening `@` in a body, the two editions after
  `@edition`, and the raw keywords on a line under a guard.
- *(tree-sitter)* The grammar reads a guard as an `edition_guard` node, with an `edition_name`
  highlighted as a built-in constant.
- *(vscode)* `@edition` is highlighted as a directive, and the edition after it as a constant.
