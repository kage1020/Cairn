- *(redstone)* An unbound signal used by a scope's own `assert` and by one nested under a `level`
  further down was reported at the nested one. It is now reported at the first in the file.

- *(redstone)* `E_LOGIC_NESTING_TOO_DEEP` claimed "about 128 chained bindings" whatever the chain,
  and stood on the binding the lowering was deepest in, the furthest from where the misordering
  starts. It now counts the bindings the lowering was inside, stands on the outermost, notes the
  next three, and counts any past those. Two chains that start from one binding still get a finding
  each.

- *(core)* An `assert truth(...)` listing one signal twice was asked for rows that give the two
  positions different values, which no circuit sees, and
  `assert truth(sig.a, sig.a -> sig.o) { -0 -> 0; -1 -> 1 }`, whose meaning depends on which of the
  two columns is read as `sig.a`, passed `check` with no finding. Such a table is now refused as
  `E_TRUTH_TABLE_DUPLICATE_INPUT`, once per repeated signal, with no other truth-table finding
  beside it.
