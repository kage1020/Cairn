- *(core)* A `place id=` or a `struct` named after a device Windows reserves passed `check` and
  was written under that name, which Microsoft's naming rules reserve whatever extension follows
  and which a Windows release applying them opens as the device. The names are `CON`, `PRN`,
  `AUX`, `NUL`, `COM0` to `COM9`, `LPT0` to `LPT9`, and `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²` and
  `LPT³`, each in any letter case. They are now refused on every host: a place id as
  `E_INVALID_PLACE_ID`, and a struct name as the new `E_INVALID_STRUCT_NAME`. A site name or a
  port id may still spell one, since it is only ever part of a walkway's file name.
  - `PlaceId::new` refuses these names with the new variant `IdError::ReservedDeviceName`. An
    external exhaustive `match` on `IdError` no longer compiles; `IdError` is now
    `#[non_exhaustive]`, so the next variant costs no second break.
  - New variant `DiagnosticCode::InvalidStructName`.
  - A lockfile recording such a place id is reported as one recording an identifier this build
    refuses, and the target it records is still compared.
  - A `connect` endpoint naming such a `place` row is `W_DEFERRED_CONNECT`, whose note points at
    that row.

  Listed here rather than as a fix under `spec/compatibility` C.4: the spec gained these names,
  rather than the build being brought in line with what it already said.
