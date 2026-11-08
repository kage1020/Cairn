- *(core)* `@edition` inside a body is no longer `E_PARSE`, so a source that writes a guard
  around a raw member now builds that member for its edition. A guard around any other member, or
  around a `logic` or `assert` line, is `E_SEMANTIC_IN_EDITION_GUARD` at that line, where the whole
  guard was `E_PARSE`.
- *(core)* `Member` gains the public field `edition_guard`, an `Option<EditionGuard>`, and
  `Resolution` the public field `edition`, and both are `#[non_exhaustive]`, so a struct literal
  of either no longer compiles outside the crate.
