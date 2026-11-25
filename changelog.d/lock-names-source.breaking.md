- *(cli)* `cairn compile --lock` refuses, with exit 2, a path whose file name ends in `.crn`,
  whether or not a file is there: a lockfile is never a Cairn source, and `other.crn` is most often
  `other.crn.lock` cut short by a shell's completion. Pass the lock under another name, such as
  `other.crn.lock`. `cairn info` still reads such a path, and its notes give that fix in place of a
  compile.
