- *(core)* `column` is a member keyword, standing `height=` blocks of its material upright from the
  cell its `at=x,y,z` names. `repeat=N step=dx,dy,dz` stands a series of `N` columns, each `step=`
  past the one before. It is laid in `massing`, its blocks take the state its material writes, and
  a column without a `mat_slot=` is built from `@column.default`. A missing `height=` or one that
  is no positive whole number, a `repeat=` that is no positive whole number, a `repeat=` above 1
  with no `step=` or with `step=0,0,0`, and a `step=` that is no triple are `E_INVALID_MEMBER`, and
  a column, any copy included, is refused or deferred where a `fill`'s box over the same cells would
  be.
- *(core)* `arch` is a member keyword, cutting the box between its `from=` and `to=` corners to air
  in `openings` and springing it from an upside-down stair at each end of its top row, facing out
  of the opening. It spans the longer of its box's `x` and `z` sides, or the `axis=x|z` its line
  writes, which a box as long on `x` as on `z` must write. `repeat=` and `step=` cut a series as
  they stand a series of columns. A block that is no stair is `E_INCOMPATIBLE_MATERIAL`, and an
  arch without a `mat_slot=` is built from `@arch.default`. A series whose copies share a cell, a
  span shorter than 2 and an unknown `axis=` are `E_INVALID_MEMBER`, and an `axis=` that is no
  identifier is `W_IGNORED_ARGUMENT`.
- *(core)* A window's `shape=` is read, and leaves Subject to change for the `.crn` syntax row's
  tier, Evolving. `shape=arch` leaves the two cells at the ends of the top row to the wall in every
  rectangle a `repeat=` series or a `sym=true` mirror cuts, and a theme's `frame=` is laid round the
  rectangle, so the two cells stay the wall's. `shape=rect`, the default, cuts the whole rectangle.
- *(formats)* Both built-in packs declare the tier-1 tokens `column.default` and `arch.default`.
- *(lsp)* Completion offers `column` and `arch` with the other member keywords.
- *(vscode)* The grammar highlights the two keywords.
