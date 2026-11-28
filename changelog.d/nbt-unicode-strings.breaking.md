- *(nbt)* `tag::check_string` and `NbtIoError::InvalidString` are removed. A string's limit
  depends on the dialect it is written in, so `check_java_string` and `check_bedrock_string` take
  its place, each refusing only with `NbtIoError::LengthOverflow`.
