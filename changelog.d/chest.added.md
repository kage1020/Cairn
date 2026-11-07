- *(core)* `chest` is a member keyword. A chest stands one chest block against a wall, in the cell
  a `pressure_plate` with the same `at=`, `offset=` and `y=` takes, refused or deferred wherever
  the plate would be. It faces away from the wall, and `facing=in|out|left|right` turns it, `left`
  being toward `offset=0`. Once every member is painted, a row of chests of one block and facing
  pairs two by two from the chests' own left as the game pairs them, an odd last chest staying
  `single`, and `size=single` keeps a chest out of every pair. `facing` and `size` are recorded in
  the chest's `intent_state`, and an unknown one is `E_INVALID_MEMBER`.
- *(core)* A chest's `mat_slot=` names its block, and a chest without one is built from the pack's
  `@chest.default`. A block whose id is neither `chest` nor ends in `_chest` is
  `E_INCOMPATIBLE_MATERIAL` with `required: "chest"`, and the chest is built from the default
  beside the error.
- *(formats)* Both built-in packs declare the tier-1 token `chest.default`, and their `connections`
  tables give chests the kind `chest`. A Bedrock build writes each half of a pair as a single
  chest with `W_INTENT_DEGRADED`, since Bedrock pairs chests through block entities no writer
  fills yet.
