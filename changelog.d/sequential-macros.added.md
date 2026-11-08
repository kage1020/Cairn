- *(core)* The built-in logic library holds the sequential macros. `latch` is called as
  `latch(set=…, reset=…)`, as `mux` is. `delay`, `pulse` and `counter` are applied to one operand
  by position and a stage count of at least 1, as in `pulse(sig.ring, 4)`, and `edge_rising` and
  `edge_falling` to one operand alone. A named operand, a missing or extra stage count, a count of
  0 or past 2^32 − 1 is `E_PARSE` where it is wrong, naming the form the macro takes.
- *(redstone)* Each macro synthesizes to one gate carrying its stage count and to one cell of
  torches and repeaters on both editions, which keeps a `latch`'s or `counter`'s feedback inside
  it, so the Logic IR stays a DAG and a signal read back through a macro is still `E_LOGIC_CYCLE`.
  A stage is two ticks: `delay(x, n)` takes `2n` ticks, `pulse(x, n)` `2n + 2`, `edge_rising` 4,
  `edge_falling` 3, a `counter` 3 and a `latch` 4 from `set` and 5 from `reset`, counted to the last
  change one change at the port makes, and `assert latency` counts them on the path. Every one restores strength.
- *(tree-sitter)* The grammar reads `latch` as a `call_expression` and the other macros as
  `apply_expression` nodes with `operand` and `stages` fields, each macro highlighted as a builtin.
