- *(nbt)* `tag::check_string` and `NbtIoError::InvalidString` are removed. A string's limit
  depends on the dialect it is written in, so `check_java_string` and `check_bedrock_string` take
  its place. Each refuses only with the new `NbtIoError::StringTooLong`, which gives the bytes the
  string takes and the encoding they are counted in. `LengthOverflow` now covers lists and arrays
  only.
