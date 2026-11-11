- *(redstone)* `verify_truth_and_temporal` checks each `assert truth` and `assert always` by
  running the legalized circuit for one edition, from rest, through every combination of the
  sensors the assertion's signals depend on, each switched on at tick 0 and held, an `observer`
  for the one tick of its pulse, each run lasting until the circuit rests again:
  - a truth table holds when, once the circuit rests, the output reads what every row whose
    pattern the inputs match says;
  - `always(a -> eventually b within N)` holds when `b` reads on within `N` ticks of every tick
    `a` does, `b` read at the torches under each actuator that reads it by that name, the slowest
    counting, or where it is driven when no actuator reads it.

  `build_circuits` and `cairn synth --stage legalize` run it after the latency check. The new
  warning `W_SIM_ASSERTION_UNCHECKED` reports an assertion left unchecked: one in a scope that
  lays no circuit, one on more than `MAX_ASSERTION_SENSORS` (10) sensors, one with a run still
  changing when its ticks run out, and the rows of a met truth table no run reaches.
