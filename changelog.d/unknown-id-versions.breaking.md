- *(core)* An `E_UNKNOWN_ID` on an id another version of the edition declares no longer carries the
  nearest spelling as `suggestion`, nor prints it, since the id is no typo; the note names the
  versions that declare it instead. `UnknownId` gains `elsewhere`, `MaterialDeferred::UnknownId`
  holds it boxed, and `DiagnosticData::UnknownId` gains `since` and `until`.
