- *(cli,lsp)* A source whose block-array lowering raises an error every target that reads its line
  raises, such as a `shed` roof bound to `@cobblestone`, now fails a `cairn check` with no
  `--target` with exit 1, as it fails `cairn compile`, instead of passing it silently; the language
  server marks it too. A check with no target also prints the lowering warnings that need no target.
  `spec/lint` "Error vs warning" states which findings a check with no target reports.

  Listed here rather than as a fix under `spec/compatibility` C.4: the spec said a `cairn check`
  with no `--target` runs no block-array lowering, so the build did what the spec said, and the spec
  changed.
