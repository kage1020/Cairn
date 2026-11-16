- *(cli,core)* `cairn info` reports, per requested edition, the interval of versions from the latest
  that first declares a block the build paints to the earliest that last declares one, as an
  `inferred range` row beside `registry compatibility` and an `inferred_range` field of `--format
  json`. It is read off pinned lowerings at the oldest and the newest version the pack carries, an
  open edge prints `0.0` or `latest`, a range no version can lie in prints `none`, and a `note:`
  names each block that sets an edge. `resolve::InferredRange`, `PaintedId`, `RequiresConflict` and
  `LastDeclaredId` carry the row.
- *(cli)* The reserved `E_REQUIRES_CONFLICT` is raised: an `@requires` floor above an edition's
  inferred range that a version the pack carries meets, so that every version it admits lacks a
  block the build paints. It names the block last declared earliest and offers that version as the
  floor. `cairn compile` refuses it whatever the `--target`, and `cairn info` prints it without
  changing the exit code. A floor above every version the pack carries is left to `E_VERSION_CAP`.
