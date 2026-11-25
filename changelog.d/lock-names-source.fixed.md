- *(cli)* `cairn compile --lock` naming the source being compiled is refused with exit 2 before the
  source is read, where the compile moved the source aside, wrote the lockfile in its place and
  exited 0, leaving no copy. Every path that reaches the source counts, whatever its spelling
  (`hut.crn`, `./hut.crn`, an absolute path, a symlinked directory, a directory that folds case)
  and a symbolic link to it, and on Linux and macOS a hard link too:

  ```text
  error: `--lock hut.crn` names `hut.crn`, the source being compiled
    note: give `--lock` a `.lock` path, or leave it out to write `hut.crn.lock`
  ```
- *(cli)* `cairn compile` writes no artifact over the source it compiles, and no lock or artifact
  through a `.tmp` or `.bak` scratch file that is the source. A source named as one of its
  artifacts (`hut.nbt` building `struct hut`) was replaced by the structure file, a source named as
  a scratch file (`notes.tmp` with `--lock notes`) was deleted, and a scratch file linked to the
  source had the artifact or the lockfile written into the source. A `--lock` whose scratch file
  is the source is refused with exit 2 before the source is read, as the source itself is; an
  artifact, or a linked `<source>.lock`, is refused with exit 1 once the build knows where its
  files go, and nothing is written:

  ```text
  error: artifact `./hut.nbt` names `hut.nbt`, the source being compiled
    note: compile with `--out` naming another directory, or give the source a name ending in `.crn`
  ```
- *(cli)* `cairn info` on a lock path `cairn compile` refuses to write, `--lock` naming the source
  or a path whose scratch file is the source, says that the compile refuses it and what to pass
  instead, where its note named the compile that would have overwritten the source as the remedy.
- *(cli)* `cairn import --out` reaching the file being imported by a hard link, or in another
  letter case where the directory folds case, is refused with exit 2 as the file's own path is. On
  Linux a name differing only in case let the import write its `.crn` over the structure file.
