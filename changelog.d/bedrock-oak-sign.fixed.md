- *(core)* A `sign` bound to `@oak_sign` or `@dark_oak_sign` builds at every Bedrock target,
  standing as `standing_sign` or `darkoak_standing_sign` and hung as `wall_sign` or
  `darkoak_wall_sign`. A wall sign the target writes under another ID, as Bedrock's state map
  writes `oak_wall_sign`, now counts as one it has, so the sign is not refused with
  `E_INCOMPATIBLE_MATERIAL`, a `cairn check` with no `--target` does not raise it on a file every
  Java target builds, and `cairn info` lists every Bedrock target as buildable. With no version
  pinned, a wall sign no shipped version writes stands in for its block and refuses no sign. A
  standing sign whose wall sign a pinned target lacks is still refused, and the message names that
  wall sign rather than calling the block no sign.
