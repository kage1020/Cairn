- *(cli)* `cairn compile` took two file names that differ only in letter case for one file on macOS
  and Windows and for two everywhere else, whatever `--out` itself did. In a case-insensitive
  directory under Linux (casefold ext4, exFAT, CIFS), a `--lock out/HOME1.NBT` beside an artifact
  `out/home1.nbt` passed the lock collision guard, and the build staged both into one file, then
  failed, losing the artifact. In a case-sensitive directory under macOS or Windows the same lock
  was refused, although there it is a file of its own. `cairn compile` now asks `--out` itself: it
  writes a uniquely named scratch file there, reads it back under its upper-case spelling, and
  removes it. Both the lock guard and the guard against two artifacts sharing one file use that
  answer. Where the probe fails, the two spellings are taken for one file. On Linux and macOS the
  lock guard also tells the lock's directory and `--out` apart by device and inode, so a
  `--lock OUT/home1.nbt` beside `--out out` is refused too when `OUT` is the same directory spelled
  another way.
