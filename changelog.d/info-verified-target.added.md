- *(cli,core)* `cairn info` reports the target the source's lockfile verified, as a
  `verified target` row and a `verified_target` field of `--format json`: the `(edition, version,
  data_version)` the lock records, whether the source still hashes to its `source_hash`, and the
  `cairn_version` that wrote it. `--lock` reads another lockfile than `<source>.lock`. A missing
  lock prints `(no lock)`, one of a newer schema `(lock schema N, newer than this Cairn reads)` and
  any other it cannot read `(unreadable lock)`, each with a `note:` naming the path and the cause,
  and none of them changes the exit code. A lock refused only for an identifier an earlier Cairn
  allowed still reports its target. `lock::LockReading::of` reads a lock as the row reports it.
