- *(core)* `UnknownId` gains the public field `absent`, `DiagnosticData::UnknownId` the field
  `declared_by`, and `UnsupportedReason` the variant `NoCounterpart`, so a struct literal of the
  first two, or an exhaustive match on the third, no longer compiles. `TargetRegistry` gains
  `absent_concept`, with a default that lists nothing.
- *(formats)* `portability_for_java` and `portability_for_bedrock` take the pack's `AbsentIndex`
  after its `AliasIndex`, and `PackFiles` gains `absent`. Both built-in packs' hashes move with the
  new component.
