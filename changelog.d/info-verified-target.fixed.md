- *(core)* A lockfile declaring `lock_schema_version: 0`, a `cairn_version` that names no Cairn
  release, or an `mc_version` holding a control character or a line break is refused as invalid,
  since no compile writes one, where it was read as written.
- *(cli)* `cairn compile` no longer warns that it is replacing a lockfile it could not read for an
  I/O reason, such as a directory at the lock path; it says the target is not compared, and the
  write that follows reports why it fails.
