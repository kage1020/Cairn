- *(core)* `@edition` inside a body is no longer `E_PARSE`, so a source that writes a guard
  around a raw member now builds that member for its edition. A guard around any other member, or
  around a `logic` or `assert` line, is `E_SEMANTIC_IN_EDITION_GUARD` at that line, where the whole
  guard was `E_PARSE`.
- *(core)* `Member` gains the public field `edition_guard`, an `Option<Edition>`, and
  `Resolution` the method `edition`. Both are `#[non_exhaustive]`, so outside the crate a struct
  literal of either no longer compiles, nor does a pattern on either written without `..`.
