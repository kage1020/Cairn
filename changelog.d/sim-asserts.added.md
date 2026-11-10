- *(redstone)* `verify_behaviour` checks each `assert truth` and `assert always` by running the
  legalized circuit for one edition, from rest, through every combination of the sensors the
  assertion's signals depend on, each switched on at tick 0 and held, an `observer` for the one
  tick of its pulse:
  - a truth table holds when the output, once the circuit rests, reads what every row its inputs
    match says;
  - `always(a -> eventually b within N)` holds when `b` reads on within `N` ticks of every tick
    `a` does, `b` read at the torches under the actuators that read it by that name.

  `build_circuits` and `cairn synth --stage legalize` run it after the latency check. An
  assertion whose signals depend on more than `MAX_DRIVEN_SENSORS`, 10, is not run and is the new
  warning `W_SIM_ASSERTION_UNCHECKED`.
