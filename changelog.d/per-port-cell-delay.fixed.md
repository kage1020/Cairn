- *(redstone)* Delay insertion charged the Java comparator AND 1 tick from either operand, though
  the operand inverted onto the comparator's side passes the inverter as well as the comparator.
  `spec/redstone` "Time model" now counts a cell's delay per input port, and "Edition differences"
  puts the AND's `a` port at the rear and inverts its `b` port onto the side, so that cell counts
  1 tick from `a` and 2 from `b`. Each cell's `local_delay_ticks` is the delay of its slowest
  driven port plus the buffer repeaters on the nets feeding it, so `cairn synth --stage delay` and
  `--stage crossing` now report a Java AND 1 tick higher. The Placement IR keeps one
  `local_delay_ticks` per cell.
