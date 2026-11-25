- *(cli)* `cairn compile --lock` naming the source being compiled is refused with exit 2 before the
  source is read, where the compile moved the source aside, wrote the lockfile in its place and
  exited 0, leaving no copy. Every path that reaches the source counts, whatever its spelling
  (`hut.crn`, `./hut.crn`, an absolute path, a symlinked directory, a directory that folds case)
  and a symbolic link to it, and on Linux and macOS a hard link too:

  ```text
  error: `--lock hut.crn` names `hut.crn`, the source being compiled
    note: give `--lock` a `.lock` path, or leave it out to write `hut.crn.lock`
  ```
- *(cli)* `cairn info --lock` naming the source says that `cairn compile` refuses the path and what
  to pass instead, where its note named the compile that would have overwritten the source as the
  remedy.
- *(cli)* `cairn import --out` reaching the file being imported by a hard link, or in another
  letter case where the directory folds case, is refused with exit 2 as the file's own path is. On
  Linux a name differing only in case let the import write its `.crn` over the structure file.
