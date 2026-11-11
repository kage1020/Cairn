- *(core)* `assert latency(sig.in -> sig.out) <= N` parses, in the core parser and the tree-sitter
  grammar. It takes `<=` and a whole number of ticks; any other comparison is `E_PARSE` naming the
  `<=` it needs.
- *(redstone)* Each `assert latency` is checked per edition against the legalized circuit: the
  slowest path from the signal's driver to each actuator reading the other signal counts the ticks
  of every cell port it enters and every buffer repeater on its routed wire. A bound it
  exceeds is `E_SIM_ASSERTION_FAILED`, with a note per cell and wire on the path and the bound
  that would hold; a bound whose end does not depend on its start is refused by synth with the
  same code on every edition. `cairn synth --experimental-logic-synth --stage legalize` runs the
  check and prints no IR when a bound fails.
- `examples/redstone-door.crn` bounds the plate-to-door path at three ticks: the `RepeaterOR`'s one
  and the two of the torches that carry the signal up to the door.
