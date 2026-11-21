- *(core)* A `theme` selector row binds the block of a part of each member it selects.
  `walls[class=outer] -> trim=@spruce_log` stands a post at each of the walls' four corners, in
  every row the member paints. `window[class=small] -> frame=@spruce_wood`, and `frame=` on a
  `door` row, lays a ring one cell wide round each opening the member cuts, each rectangle of a
  `repeat=` series or `sym=true` mirror its own. A frame is laid once every opening is cut and
  every door hung, only on cells the walls still hold, clipped to the opening's wall course and off
  the wall's end columns, so it never closes another opening, turns a corner, covers a corner post
  or changes a hinge. The part is its member's: laid in its phase and owned by it, and two frames
  of different blocks on one cell are a `W_PHASE_CONFLICT` that names the two frames.
- *(core)* A binding's value resolves as a `slot` rule's value does, against the compile's target:
  a value that is no material token is `E_UNKNOWN_SLOT_TARGET`, and a block the target cannot build
  earns the finding a slot's value would (`E_UNKNOWN_ID`, `E_UNKNOWN_ABSTRACT_TOKEN`,
  `W_ABSTRACT_TOKEN_DEFERRED`, `E_PARITY_UNSUPPORTED`, `E_STATE_DOMAIN` or `E_STATE_UNMAPPED`), at
  the binding, and its member is built without the part. `MemberRole::binding_keys` and
  `BindingKey` name the keys each keyword binds.
- *(lsp)* Completion right of a theme selector row's arrow offers the keys its keyword binds, less
  any the row binds already.
- *(lsp)* Hover on a `@` token a selector row binds shows the block it names on Java and on
  Bedrock, as a `slot` row's does, or says it lays nothing when the row's keyword does not bind its
  key.
