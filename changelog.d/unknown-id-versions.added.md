- *(core,formats)* `E_UNKNOWN_ID` on a block the pinned version lacks but another version of the
  edition declares names the nearest targets that declare it: the first after the pinned one,
  offered as a `--target` and as the upper bound of an `@requires <edition> version>=` floor, and
  the last before it, where a dropped block was last declared. `@pale_oak_planks` on Java 1.21 is
  answered with 1.21.4, and `@chain` on Java 26.3 with 1.21.4 beside the alias table's
  `iron_chain`. The payload carries them as `since` and `until`, and a `TargetRegistry` answers
  them through `declared_elsewhere`.
