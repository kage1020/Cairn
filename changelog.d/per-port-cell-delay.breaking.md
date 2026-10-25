- *(redstone)* `EditionCell::base_delay_ticks()` is replaced by
  `EditionCell::port_delay_ticks(PortName)`, the ticks from one input port to the cell's output.
  Every cell but the Java comparator AND counts what it did on each of its ports; that one counts
  1 from `PortName::A` and 2 from `PortName::B`. The figure delay insertion charges a cell is the
  largest of these over the ports its drivers feed.
