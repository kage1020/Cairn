- *(core)* A struct's block entities were listed in the order their members painted them, so
  swapping two `sign`, `bed` or `chest` lines that share no voxel changed `resolved_ir_hash` on
  both editions and the bytes of the `.mcstructure`, although the built structure was the same.
  They are now listed in the order of their cells, a laid circuit's included, as the palette and
  the entities already are. This moves `resolved_ir_hash` for a build whose block entities were not
  already in that order, so its next `cairn compile` rewrites the lockfile.
