- *(core)* `sign` is a member keyword. By a wall, with `at=`, `offset=` and `y=` as a `chest`
  takes them, it hangs on the wall's face as a wall sign facing away from it; at a coordinate
  triple it stands on the block below toward the front wall, or hangs on the block behind it when
  `facing=` names a compass direction. `text=` writes its front and `back=` its back, each one
  string or a list of up to four, and `glowing=true` lights both. Its `mat_slot=` names a standing
  sign, which hangs as the wall sign of the same wood, and a sign without one is built from
  `@sign.oak`.
- *(core)* `bed` is a member keyword, laying a bed's head and foot as two blocks with `part` and
  `facing`. By a wall its head takes the cell a `chest` would and lies against the wall, its foot
  behind it, and `facing=left|right` turns it along the wall; at a coordinate triple `facing=` is
  a compass direction and must be written. A foot in the walls or past the struct is
  `E_INVALID_MEMBER`. A block that is no bed is `E_INCOMPATIBLE_MATERIAL` with `required: "bed"`,
  and a bed without a `mat_slot=` is built from `@bed.default`, a red bed.
- *(core)* A sign's text and a bed's colour ride in the block array as block entities beside their
  cells, and a member that builds over one later takes it away. A member of the same phase that
  builds the same block over one with another block entity, such as a sign with other text, is
  `W_PHASE_CONFLICT`, and a fixture that takes one of a bed's two cells is told it leaves half a
  bed. `TargetRegistry::canonical_block` tells the lowering which Java bed a Bedrock `bed` token
  stands for.
- *(core)* `block_array` gains `SignText`, a face of at most `SIGN_FACE_LINES` lines, and
  `DyeColor`, the sixteen dyes in the order the game numbers them.
- *(formats)* The `.nbt`, `.litematic` and `.schem` writers save a sign's faces as `front_text`
  and `back_text`, its messages JSON strings before 1.21.5 and plain strings from it. The
  `.mcstructure` writer saves a sign's faces as `FrontText` and `BackText`, a bed's colour as the
  dye's number, each half of a chest pair with the cell of its other half, which keeps the pair
  joined, and a note block's `note`. A chest half with no other half beside it stands single on
  Bedrock and is reported as `W_INTENT_DEGRADED`. Every writer refuses sign text the NBT cannot
  carry, naming the sign's cell: `JavaStructureError` and `BedrockStructureError` gain
  `UnencodableSignText`.
- *(formats)* Both built-in packs declare `bed.default` and a `bed.<colour>` token for each of the
  sixteen dyes. Bedrock's resolve to its one `bed` block and name the Java bed each stands for with
  the new `canonical` field of a `materials` entry. A pack load refuses a `canonical` the state map
  does not map onto the entry's block, and any `canonical` in a Java pack, as `RegistryError`'s new
  `CanonicalUnmapped`, `CanonicalWithoutStates` and `CanonicalOnJava`.
- *(lsp)* Completion offers `sign` and `bed` with the other member keywords.
- *(vscode)* The grammar highlights the two keywords.
- `examples/workshop.crn` lays a brown bed against its back wall and hangs a sign by its door.
