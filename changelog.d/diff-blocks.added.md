- *(cli)* `cairn diff-blocks STRUCTURE FILE` builds a lift at the target the structure file is
  stamped with and compares the two cell by cell, closing the loop `cairn import` opens. It prints
  the block IoU, the `state_accuracy` and the share of the build raw members still build, each
  against its threshold (≥ 0.985, ≥ 0.995, ≤ 5%); every region of cells that are missing, extra,
  replaced or in another state, with its block and the member that likely caused it; and up to
  three `edit` lines, each found by building the source with it appended, that bring the build
  nearer the file. Short of convergence it exits 1 with `E_DECOMPILE_DIFF` on stderr. `--format
  json` prints the whole report, every region included, as one document. A source that does not
  build at the stamp is refused before anything is compared.
- *(core)* `voxel_diff` compares a structure's grids with a build: `compare` counts the cells each
  side holds a block at, the matched ones and the residual ones, `Counts` turns them into
  `block_iou`, `state_accuracy` and `residual_ratio`, `regions` clusters what differs and names a
  cause, `suggest_patch` searches for the `edit` lines, and `diff` runs all three. A Bedrock
  property either side leaves out reads as its state map's default, and `waterlogged` as `false`.
