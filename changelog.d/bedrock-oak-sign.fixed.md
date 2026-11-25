- *(core)* A `sign` bound to `@oak_sign` or `@dark_oak_sign` was refused at every Bedrock target,
  by a `cairn check` with no target, and in `cairn info`. It now stands as `standing_sign` or
  `darkoak_standing_sign` and hangs as `wall_sign` or `darkoak_wall_sign`.
- *(core)* With no version pinned, a wall sign no shipped version writes stands in for its block,
  and no sign is refused for it.
- *(core)* A standing sign whose wall sign a pinned target lacks is still refused, and the message
  names that wall sign.
