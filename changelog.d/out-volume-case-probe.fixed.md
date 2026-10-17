- *(cli)* `cairn compile` took two file names that differ only in letter case for one file on macOS
  and Windows and for two everywhere else, whatever the volume under `--out` actually did. On a
  case-insensitive mount under Linux (casefold ext4, exFAT, CIFS), a `--lock out/HOME1.NBT` beside
  an artifact `out/home1.nbt` passed the lock collision guard, so the build wrote both to one file.
  On a case-sensitive volume under macOS or Windows the same lock was refused, although there it is
  a file of its own. `cairn compile` now asks the volume: it writes a uniquely named scratch file
  into `--out`, reads it back under its upper-case spelling, and removes it. Both the lock guard and
  the guard against two artifacts sharing one file use that answer. Where the probe cannot run, the
  platform's default is used as before.
