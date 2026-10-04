- *(redstone)* An unbound signal used by a scope's own `assert` and by one nested under a `level`
  further down was reported at the nested one. It is now reported at the first in the file.

- *(redstone)* `E_LOGIC_NESTING_TOO_DEEP` claimed "about 128 chained bindings" whatever the chain,
  and stood on the binding the lowering was deepest in, which is the one not misordered. It now
  counts the bindings the lowering was inside, stands on the outermost, and notes the next few.

- *(core)* An `assert truth(...)` listing one signal twice was asked for rows that give the two
  positions different values, which no circuit sees. It is now refused as
  `E_TRUTH_TABLE_DUPLICATE_INPUT`, with no other truth-table finding beside it.
