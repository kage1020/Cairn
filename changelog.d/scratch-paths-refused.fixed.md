- *(cli)* `cairn compile` and `cairn import` wrote each file at its path with `.tmp` added and
  moved the file it replaced to its path with `.bak` added without checking either name. A user's
  `hut.nbt.bak` or `hut.crn.lock.tmp` was deleted in silence, and a symbolic link at `.tmp` had the
  file written into whatever it pointed at. An entry at either scratch path now refuses the run
  with exit 1 before any artifact, lockfile or source is written, as it already refused
  `cairn install`, and the refusal names each entry to move aside. The scratch file is created
  only where nothing is. A scratch file a run leaves behind, such as a `.bak` it could not remove,
  now refuses the next run too, so the warning that reports it says to remove it first.
