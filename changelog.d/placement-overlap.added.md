- *(core)* Once the coordinate solver has placed every `place` row of a site, lowering compares
  each pair of their boxes, and each walkway strip against every placement of its site but its two
  ends. Two rows whose boxes share a cell, or a strip that lays a cell inside the box of a third
  placement, as under that placement's eaves, are `E_PLACEMENT_OVERLAP`. A pair of rows is
  reported with both boxes, the box they share and, for a relative row, the `gap=` that moves it
  clear, and a strip with the placement's box and the box around the cells it lays there. Each
  structure file writes air where its body is empty, so two that overlap decide each other's
  shared cells by the order they are placed in. Between bodies the check holds only in part: the
  air a walkway's file writes beyond its strip, a strip under the eaves of a placement it joins,
  and two walkways against each other are not compared yet.
