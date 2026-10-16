- *(core)* A `place id=` or a `struct` named after a device Windows reserves passed `check` and
  built on Linux and macOS, and failed on Windows with a bare OS error when its artifact was
  written. The names are `CON`, `PRN`, `AUX`, `NUL`, `COM0` to `COM9` and `LPT0` to `LPT9` in any
  letter case, and `COM¹`, `COM²`, `COM³`, `LPT¹`, `LPT²` and `LPT³`. They are now refused on every
  host: a place id as `E_INVALID_PLACE_ID`, and a struct name as the new `E_INVALID_STRUCT_NAME`.
  A site name or a port id may still spell one, since it is only ever part of a walkway's file
  name.
  - `PlaceId::new` refuses these names with the new variant `IdError::ReservedDeviceName`.
    `IdError` is not `#[non_exhaustive]`, so a `match` on it needs the new arm.
  - New variant `DiagnosticCode::InvalidStructName`.
  - A lockfile recording such a place id is reported as one recording an identifier this build
    refuses, and the target it records is still compared.
  - A `connect` endpoint naming a `place` row refused for its id is `W_DEFERRED_CONNECT`, whose note
    points at that row, rather than `E_UNRESOLVED_PLACE_REF`.

  Listed here rather than as a fix under `spec/compatibility` C.4: the spec gained these names,
  rather than the build being brought in line with what it already said.
