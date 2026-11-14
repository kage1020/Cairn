- *(core)* Once the coordinate solver has placed every `place` row of a site, lowering compares
  each pair of their boxes, and each walkway strip against every placement but its two ends. Two
  rows whose boxes share a cell, or a strip that lays a cell inside a third placement, as under its
  eaves, are `E_PLACEMENT_OVERLAP`. The finding names both boxes and the box they share, and for a
  relative row the `gap=` that moves it clear. Each structure file writes air where its body is
  empty, so two that overlap decide each other's shared cells by the order they are placed in.
