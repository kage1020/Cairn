- *(redstone)* `EditionCell::base_delay_ticks()` is replaced by
  `EditionCell::port_delay_ticks(PortName)`, the ticks from one input port to the cell's output.
  Every cell but the Java comparator AND returns from each port what `base_delay_ticks` returned;
  that one returns 1 from `PortName::A` and 2 from `PortName::B`. Delay insertion charges a cell the
  largest of these over the ports its drivers feed.
