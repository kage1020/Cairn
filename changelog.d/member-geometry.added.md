- *(core)* Lowering records each member's geometry beside its `resolved_state`: the cell its line
  places it by, the box its line declares, the box around every cell it holds once its body is
  painted, and the face of the wall or wall top it is set against. A roof declares its struct's
  footprint and holds its overhang past it, a window declares every rectangle its line places,
  mirror included, and a member a later phase took cells from keeps its declared box while its held
  box shrinks. `ResolvedMember::geometry` carries the four as a `MemberGeometry`, its boxes as
  `Bbox`, so paintings, item frames and an entity-AABB check can be placed and judged from them, and
  `cairn lower --format json` prints them under each member's `geometry`.
- *(core)* A window's `anchor=` names the cell its `offset=` and `y=` place, one of `bottom_left`,
  `bottom_center`, `bottom_right`, `middle_left`, `center`, `middle_right`, `top_left`, `top_center`
  and `top_right`, seen from outside the wall, so `anchor=bottom_center offset=4 size=3x3` cuts the
  columns 3 to 5. Its mirror, its `repeat=` series and its `connect` port follow the rectangle the
  anchor places.
- *(core)* A roof reads `footprint=struct` and `bounds=expand`, the one geometry each names: the
  roof covers its struct's `size=` footprint, and its `overhang=` widens the body's block array past
  it.
