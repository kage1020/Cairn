- *(lsp)* `textDocument/codeAction` answers quick-fixes built from each diagnostic's `data`, never
  from its message: for an authored `E_UNKNOWN_ID`, each alias the pack declares, the first one
  preferred, then the nearest id; for `E_UNKNOWN_KEYWORD` and `E_UNKNOWN_ARGUMENT`, the keyword or
  key the note suggests; for `E_VERSION_CAP`, lowering the floor to the target, or, for a client
  that declares the `cairn.setTarget` command, pinning the lowest target the floors admit. A
  diagnostic whose range no longer holds what its payload names gets no fix.
- *(lsp)* The `cairn.edition` and `cairn.target` settings, read from
  `workspace/didChangeConfiguration`, pin what diagnostics are checked against: an edition alone as
  `cairn check --edition` checks, both as `cairn compile --target` does, `@requires` floors
  included, so `E_UNKNOWN_ID`, `E_STATE_DOMAIN`, `E_VERSION_CAP` and the other codes that need a
  target reach the editor. A setting the CLI would refuse is shown with `window/showMessage`, and a
  new pin re-reports every open document.
- *(core)* `E_UNKNOWN_KEYWORD` and `E_UNKNOWN_ARGUMENT` carry a `data` payload with the word as
  written and the suggestion their note offers, and `E_VERSION_CAP` one with the edition, the
  target, the floor and the targets the floors admit. `DiagnosticData` deserializes, and
  `check::weigh_target` holds a target to a module's floors for every front end.
- *(spec)* `spec/lint` describes the language server's pin, the three payloads and the quick-fixes
  read from them, in English and Japanese.
