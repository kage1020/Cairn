- *(cli)* `cairn diff-blocks STRUCTURE FILE` builds a lift at the target the structure file is
  stamped with and compares the two cell by cell, closing the loop `cairn import` opens. It prints
  the block IoU, the `state_accuracy` and the share of the build raw members still build, each
  against its threshold (≥ 0.985, ≥ 0.995, ≤ 5%); the regions of cells that are missing, extra,
  replaced or in another state, with their block and the member that likely caused each, the first
  ten of them; and up to three `edit` lines, each found by building the source with it appended,
  that bring the build nearer the file. Only blocks are compared, and a note says so when the file
  holds block entity data or entities. Short of convergence it exits 1 with `E_DECOMPILE_DIFF` on
  stderr; `--format json` prints the whole report, every region included, as one document whose
  `code` carries it instead. A source that does not build at the stamp, or builds what the target's
  backend cannot write, is refused before anything is compared.
- *(core)* `voxel_diff` compares a structure's grids with a build: `compare` counts the cells each
  side holds a block at, the matched ones and the residual ones, `Counts` turns them into
  `block_iou`, `state_accuracy` and `residual_ratio`, and `diff` clusters what differs into regions
  with a likely cause and searches for the `edit` lines that close them. A site is compared as one
  body laid from its arrays, however far apart its placements stand, and a source of one body is
  compared with a `.litematic`'s regions laid at their corners, so the file a site compiles to
  compares with that site. A Bedrock property either side leaves out reads as its state map's
  default, and `waterlogged` as `false`.
