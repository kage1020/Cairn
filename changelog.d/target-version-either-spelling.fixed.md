- *(formats)* `--target` matched a version label exactly, so `--target 1.21.0` on Java was refused
  with "did you mean `1.21.4`?", a different release, while `@intended_targets ["1.21.0"]` was read as
  Java 1.21. `--target` now names a row by any spelling of its version, trailing zeros ignored, as
  `@requires` and `@intended_targets` already did; the target and the lockfile carry the row's own
  label (`1.21` on Java, `1.21.0` on Bedrock).
