- *(cli)* `cairn compile`, `check`, `info` and `lower --edition` refuse a circuit an `assert truth`
  or `assert always` fails on, with `E_SIM_ASSERTION_FAILED`, so a source whose truth table its
  logic contradicts, or whose `within` bound its circuit misses, no longer builds.
