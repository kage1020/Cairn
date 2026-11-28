- *(cli)* `cairn compile` and `cairn import` wrote each file at its path with `.tmp` added and
  moved the file it replaced to its path with `.bak` added without checking either name, so a
  user's `hut.nbt.bak` or `hut.crn.lock.tmp` was deleted in silence, and a symbolic link at
  `.tmp` had the structure written into its target, outside `--out`. An entry at either scratch
  path now refuses the run, exit 1, before any file is written, naming each entry to move aside,
  as `cairn install` already did; the scratch file is created only where nothing is.
