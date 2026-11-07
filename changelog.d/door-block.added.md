- *(core)* A `door` sets a door block into the opening it cuts, two rows tall, rather than leaving
  the opening as air. The block faces into the struct, is closed, and is hinged from the wall
  beside it once every opening is cut: on the right with a door on its left and none on its
  right, or with more wall on its right than its left, and on the left otherwise, left and right
  as seen from outside. `hinge=left|right` and `open=true|false` on the door's line replace what
  is derived and are recorded in its `intent_state`, and an unknown `hinge=` is
  `E_INVALID_MEMBER`.
- *(core)* A door's `mat_slot=` names its door block, and a door without one is built from the
  pack's `@door.default`, `oak_door` on Java and `wooden_door` on Bedrock. A block outside the
  door family is `E_INCOMPATIBLE_MATERIAL` with `required: "door"`, and the door is built from the
  default beside the error.
- *(formats)* Both built-in packs declare the tier-1 token `door.default`.
