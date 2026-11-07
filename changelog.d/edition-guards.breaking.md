- *(core)* `@edition` inside a body is no longer `E_PARSE`, so a source that writes a guard
  around a raw member now builds that member for its edition.
- *(core)* `Member` gains the public field `edition_guard`, an `Option<EditionGuard>`, and
  `Resolution` the public field `edition`, so a struct literal of either no longer compiles.
