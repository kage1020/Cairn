- *(core)* `fill` is a member keyword. Its `from=` and `to=` are two opposite corners of a box,
  coordinate triples measured from the footprint's low corner and written in either order. With a
  `mat_slot=`, the fill paints every cell of the box with that material in `massing`, where
  overlapping another massing member is `W_PHASE_CONFLICT`. With `fluid=water`, and `kind=source`
  or no `kind=`, it pours water in `fluids`, a phase after `fixtures`: a cell holding no block takes
  `minecraft:water[level=0]`, a block the target says can hold water takes `waterlogged=true` and
  stays its own member's, and any other block is left as it was painted. A corner outside the
  struct, another fluid or kind, or both `mat_slot=` and `fluid=` is `E_INVALID_MEMBER`, and a box
  reaching rows the struct as written reaches but does not build is `W_DEFERRED_MEMBER`.
- *(core)* `waterlogged=auto|true|false` on a `stair` or a `chest`. `true` and `false` are written
  into the block's state and kept wherever water overlaps it, `auto` leaves it to a water fill as
  no `waterlogged=` does, and any other word is `E_INVALID_MEMBER`. A chest records it in its
  `intent_state`, as a stair already did.
- *(core)* `TargetRegistry::holds_water` says whether a block takes `waterlogged=true` at the target.
  It defaults to `None`, under which a water fill waterlogs no block.
- *(formats)* The built-in packs answer `holds_water` from each block's `waterlogged` state, a view
  with no version pinned reading a block as every version that declares it agrees. A Bedrock build
  holds a waterlogged block's water in the structure's second layer.
- `examples/aquarium.crn` paints a sand bed and pours water around the panes and chests of a tank.
