- *(redstone)* Delay insertion held the wire into the Java comparator AND's side, port `b`, to what
  the cell can spare of the dust its wire out needs, and counted the dust spent on it toward that
  wire out, though the operand there ends at the inverter, which restores strength. `spec/redstone`
  "Place-and-route" now says only the comparator's rear runs on through the cell: the side's wire
  may carry all 15 blocks of dust, as the wire into any sink that restores strength may, and the
  wire out starts from what the rear has spent. A Java AND fed over a long or winding wire on its
  side takes fewer buffer repeaters and is refused with `E_ATTENUATION_LIMIT` less often, so
  `cairn synth --stage delay` and `--stage crossing` can report fewer `buffer_coords` and a lower
  `local_delay_ticks` for that cell and the ones it feeds. An `E_ATTENUATION_LIMIT` refusal of a
  wire held to a cell's budget now names the port it feeds, such as port `a`, and no longer says a
  cell passes on the strength of an operand it restores.
- *(redstone)* `EditionCell::port_regenerates(PortName)` says whether a cell restores the strength
  of a signal entering by one port: every port of a cell that `regenerates()`, and port `b` of the
  Java comparator AND.
