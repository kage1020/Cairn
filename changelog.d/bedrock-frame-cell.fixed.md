- *(core)* A Bedrock build refuses an `item_frame` hung in a cell a member has built a block in, or
  another frame hangs in, with the new `E_FRAME_CELL_TAKEN` at the frame's line and a note at the
  line that holds the cell. `cairn check --edition bedrock`, the language server and `cairn info`
  refuse what `cairn compile` refuses, instead of the `.mcstructure` writer alone refusing it with
  no code or line.
