- *(cli)* `cairn info` prints seven rows where it printed six, the new `inferred range` second, so
  a pipeline that reads the rows by position reads one more.
- *(cli)* `cairn compile --target` below a floor that `E_REQUIRES_CONFLICT` refuses reports the
  conflict in place of `E_VERSION_CAP`; the build was refused before and still is.
- *(core)* `resolve::EditionReport` gains `inferred_range`, so code that builds one sets it.
