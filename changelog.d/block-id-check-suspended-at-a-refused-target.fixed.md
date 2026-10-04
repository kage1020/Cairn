- *(cli)* `cairn compile --target` below a declared `@requires` floor was reported as `E_UNKNOWN_ID`
  instead of `E_VERSION_CAP` when the target also lacked one of the file's blocks, which is the usual
  reason to declare the floor. The author was told to replace the block and never that the target was
  below the floor. The same `E_UNKNOWN_ID` stood in for `E_REQUIRES_UNORDERABLE` when a floor named a
  version the edition's table cannot place, which refuses every target. A target the floors refuse
  now lowers with no version pinned, so no id is checked at all until the target clears every floor,
  a typo no version declares included, and the build is refused with `E_VERSION_CAP` or
  `E_REQUIRES_UNORDERABLE`. Findings that need no version, such as `E_INVALID_REQUIRES` and
  `E_UNKNOWN_ABSTRACT_TOKEN`, are still reported ahead of the refusal.
