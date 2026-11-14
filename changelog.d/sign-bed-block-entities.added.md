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
  cells, and a member that builds over one later takes it away. `TargetRegistry::canonical_block`
  tells the lowering which Java bed a Bedrock `bed` token stands for.
- *(formats)* The `.nbt` writer saves a sign's faces as `front_text` and `back_text`, its messages
  JSON strings before 1.21.5 and plain strings from it. The `.mcstructure` writer saves a sign's
  faces as `FrontText` and `BackText`, a bed's colour as the dye's number, each half of a chest pair
  with the cell of its other half, which keeps the pair joined, and a note block's `note`. Either
  writer refuses sign text outside ASCII, naming the sign's cell.
- *(formats)* Both built-in packs declare `bed.default` and a `bed.<colour>` token for each of the
  sixteen dyes. Bedrock's resolve to its one `bed` block and name the Java bed each stands for with
  the new `canonical` field of a `materials` entry.
- *(lsp)* Completion offers `sign` and `bed` with the other member keywords.
- *(vscode)* The grammar highlights the two keywords.
- `examples/workshop.crn` lays a brown bed against its back wall and hangs a sign by its door.
