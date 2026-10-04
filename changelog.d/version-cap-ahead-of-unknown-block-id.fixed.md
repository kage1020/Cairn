- *(cli)* `cairn compile --target` below a declared `@requires` floor was reported as `E_UNKNOWN_ID`
  instead of `E_VERSION_CAP` when the target also lacked one of the file's blocks, which is the usual
  reason to declare the floor. The author was told to replace the block and never that the target was
  below the floor. A target below a floor now lowers without checking block ids against it, so the
  build is refused with `E_VERSION_CAP` alone; findings that do not depend on the target still come
  first.
