- *(core)* The built-in logic library holds the sequential macros. `latch` is called as
  `latch(set=…, reset=…)`, as `mux` is. `delay`, `pulse` and `counter` are applied to one operand
  by position and a stage count from 1 to 2^32 − 1, as in `pulse(sig.ring, 4)`, and `edge_rising`
  and `edge_falling` to one operand alone. A named operand, a missing or extra stage count, a
  count without its comma, of 0 or past 2^32 − 1 is `E_PARSE` where it is wrong, naming the form
  the macro takes.
- *(redstone)* Each macro synthesizes to one gate, carrying its stage count where it counts one,
  and to one cell of torches and repeaters on both editions. A `latch` or `counter` keeps its
  feedback inside its cell, so the Logic IR stays a DAG and a signal read back through a macro is
  still `E_LOGIC_CYCLE`. A stage of `delay` or `pulse` is two ticks: `delay(x, n)` takes `2n`
  ticks, `pulse(x, n)` `2n + 2`, `edge_rising` 4, `edge_falling` 3, a `counter` 3 and a `latch` 4
  from `set` and 5 from `reset`, counted to the last change one change at the port makes, and
  `assert latency` counts them on the path. Every one restores strength. A Placement IR dump whose
  cell counts no stage, carries an unknown field or is driven on a port it does not have is
  refused.
- *(tree-sitter)* The grammar reads `latch` as a `call_expression` and the other macros as
  `apply_expression` nodes with an `operand` field, and a `stages` field on those that count
  stages, each macro highlighted as a builtin.
