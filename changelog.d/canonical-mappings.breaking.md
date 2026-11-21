- *(cli)* `cairn info` prints eight rows where it printed seven, the new `respelled` fourth, so a
  pipeline that reads the rows by position reads one more.
- *(cli)* A canonical token a pinned version writes under another spelling is counted as that
  spelling, so it no longer bounds `inferred range` nor raises `E_REQUIRES_CONFLICT`, and a Java
  block Bedrock writes as a block of its own counts as `portable` in `edition portability` where it
  counted as `unsupported`.
- *(core)* `resolve::EditionReport` gains `respelled`, so code that builds one sets it, and
  `findings_without_target` and `findings_under_guards` take a closure that answers an
  `IdStanding` where it answered a `bool`.
- *(formats)* `RegistryPack::may_declare` becomes `RegistryPack::id_standing`, which also says
  whether some version writes the id under another one.
- *(formats)* `PackFiles` gains `meanings`, and both built-in packs' hashes move with the new
  component.
