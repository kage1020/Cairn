- *(core)* A member line refused as `E_UNKNOWN_KEYWORD`, such as a misspelled `flor`, or as
  `E_MISPLACED_MEMBER`, such as a `connect` in a `struct` or `def` body, also drew a
  `W_DEFERRED_MEMBER` saying the keyword was not yet handled by block-array lowering, as if it were
  a feature to come. The refusal is now the line's only finding.
