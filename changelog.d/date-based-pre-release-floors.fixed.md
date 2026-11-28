- *(core)* A `@requires` or part-level `requires` floor at a pre-release spelled the way Mojang's
  date-based versions are, such as `26.3-rc-1`, `26.3-pre-1` or `26.3-snapshot-1`, was refused as
  `E_INVALID_REQUIRES`. A pre-release tag may now separate its runs with `-` as well as `.`, and a
  floor whose tag follows a release's number, a snapshot's among them, is placed at that release.
- *(core)* A pre-release tag now orders run by run, split at each `.` and `-` and where letters
  meet digits, a run of digits by its value, so `26.3-rc-2` sorts below `26.3-rc-10` and
  `1.13-pre2` below `1.13-pre10`, and `1.21.4-rc1` orders with `1.21.4-rc-1`.
