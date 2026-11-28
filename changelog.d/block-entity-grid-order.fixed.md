- *(core)* A struct's block entities were listed in the order their members painted them, so
  swapping two `sign`, `bed` or `raw_block` with `nbt=` lines that share no voxel changed
  `resolved_ir_hash` on both editions and the bytes of the `.mcstructure`, although the built
  structure was the same. They are now listed in the order of their cells, a laid circuit's
  included, as the entities already are. This moves `resolved_ir_hash` for a build whose block
  entities were not already in that order, so its next build writes a different
  `resolved_ir_hash` than its lock holds.
