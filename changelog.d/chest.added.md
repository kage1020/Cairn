- *(core)* `chest` is a member keyword. A chest stands one chest block against a wall, in the cell
  a `pressure_plate` with the same `at=`, `offset=` and `y=` would take, refused or deferred
  wherever the plate would be. It faces away from the wall, and `facing=in|out|left|right` turns
  it, `left` being toward `offset=0`. Once every member is painted, a row of chests of one block
  and facing pairs two by two from the chests' own left, an odd last chest staying `single`, and
  `size=single` keeps a chest out of every pair. `facing` and `size` are recorded in the chest's
  `intent_state`. An unknown identifier for either is `E_INVALID_MEMBER`, and a value that is no
  identifier is `W_IGNORED_ARGUMENT`.
- *(core)* A chest's `mat_slot=` names its block, and a chest without one is built from the pack's
  `@chest.default`. A block the pack does not read as a chest, such as an ender chest, is
  `E_INCOMPATIBLE_MATERIAL` with `required: "chest"`, and the chest is built from the default
  beside the error. Where the pack cannot say, a block whose id is `chest` or ends in `_chest` is
  taken as a chest.
- *(formats)* Both built-in packs declare the tier-1 token `chest.default`, and their `connections`
  tables give chests the kind `chest`.
