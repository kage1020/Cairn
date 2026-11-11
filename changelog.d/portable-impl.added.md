- *(redstone)* `verify_portable` reads each struct as built for an edition and refuses with
  `E_NO_PORTABLE_IMPL` the two shapes that depend on block-update order:
  - on every edition, an `observer` with a `->` tail whose front watches a `lamp`, `piston`,
    `dispenser` or `door` a binding drives, or the cell a driven piston's head moves into. It is
    reported at the observer, with a note at the binding and a fix naming the order-free
    `edge_rising(sig.x) or edge_falling(sig.x)`;
  - on Java, a driven `piston` or `dispenser` that a sensor powers through the cell above it and
    not directly, by giving off a signal into that cell or by powering whole a block beside it,
    which any block but air, a sensor, a piston and a door passes on. It is reported at the
    actuator, with a note at the sensor.

  Tier 0 parts and raw members are not checked.
- *(cli)* `cairn synth --experimental-logic-synth` runs the check at every edition-tagged stage and
  prints no IR when it refuses. It reads the members that build for the edition, and leaves one
  that does not to `cairn compile`, which reports it.
