- *(core)* The built-in logic library holds `xor`, `nand` and `nor`, infix, and `mux`, called as
  `mux(sel=…, a=…, b=…)` with every operand named once in any order. `and` and `nand` group first,
  then `xor`, then `or` and `nor`, the order of the bitwise operators. A call that passes an operand
  by position, leaves one out, names one twice or names one `mux` does not take is `E_PARSE`, and
  the message names the operands still to name.
- *(redstone)* Each new primitive synthesizes to its gate and its cell on both editions, built from
  torches: `nand` two inverters onto a common line, `nor` one torch on a block both operands power,
  `xor` and `mux` two torch NORs onto a common line. `nand` and `nor` take 1 tick from each port,
  `xor` and `mux` 2, and every one restores strength, so the wire into each of its ports ends there.
- *(tree-sitter)* The grammar reads the new primitives with the same ranks, and `mux` calls as
  `call_expression` nodes holding a `named_operand` per operand, its `operand_name` highlighted as
  a parameter.
