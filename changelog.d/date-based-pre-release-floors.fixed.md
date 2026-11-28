- *(core)* A `@requires` or part `requires` floor at a pre-release spelled the way Mojang's
  date-based versions are, such as `26.3-rc-1`, `26.3-pre-1` or `26.3-snapshot-1`, was refused as
  `E_INVALID_REQUIRES`. A pre-release tag may now separate its runs with `-` as well as `.`, and a
  tag after a release core, a snapshot's included, places the floor at that release.
- *(core)* Labels whose pre-release tags differ only in a number now order by that number, so
  `26.3-rc-2` sorts below `26.3-rc-10`.
