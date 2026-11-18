- *(lsp)* `textDocument/codeAction` answers quick-fixes built from each diagnostic's `data`, never
  from its message: for an authored `E_UNKNOWN_ID`, each alias the pack declares, the first one
  preferred, then the nearest id; for `E_UNKNOWN_KEYWORD` and `E_UNKNOWN_ARGUMENT`, the keyword or
  key the note suggests; for `E_VERSION_CAP`, lowering the floor to the target and, for a client
  that declares the `cairn.setTarget` command, also pinning the lowest target the floors admit. A
  diagnostic whose range no longer holds what its payload names gets no fix.
- *(lsp)* The `cairn.edition` and `cairn.target` settings, read from
  `workspace/didChangeConfiguration`, pin what diagnostics are checked against: an edition alone as
  `cairn check --edition` checks, both as `cairn compile --target` does, the file's `@requires` and
  `requires` floors included, so `E_UNKNOWN_ID`, `E_STATE_DOMAIN`, `E_VERSION_CAP` and the other
  codes that need a target reach the editor. Every pin weighs `@intended_targets` as the CLI does. A
  setting the server cannot read is shown with `window/showMessage`, and a new pin re-reports every
  open document.
- *(core)* `E_UNKNOWN_KEYWORD` and `E_UNKNOWN_ARGUMENT` carry a `data` payload with the word as
  written and the suggestion their note offers, and `E_VERSION_CAP` one with the edition, the
  target, the floor and the targets the floors admit. `DiagnosticData` deserializes. For every
  front end, `check::weigh_target` holds a target to a module's floors, and
  `check::check_with_intended_targets` merges the `@intended_targets` findings into `check`'s.
- *(formats)* `resolve_target` resolves a `--target` in either edition's built-in table to a
  `ResolvedTarget`, and `builtin_version_tables` hands the built-in packs' version tables to core's
  passes.
- *(spec)* `spec/lint` describes the language server's pin, the three payloads and the quick-fixes
  read from them, in English and Japanese.
