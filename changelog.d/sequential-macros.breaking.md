- *(core)* `latch`, `delay`, `pulse`, `edge_rising`, `edge_falling` and `counter` are library
  words, so a source whose `logic` operand is a signal named by one of them, such as `latch.x`, or
  the bare word, no longer parses. A dotted name may still end in one, as in `sig.delay`.
- *(core)* `LogicPrimitive` gains `Latch`, `Delay`, `Pulse`, `EdgeRising`, `EdgeFalling` and
  `Counter`, `Notation` gains `Apply`, and `LogicPrimitive::ALL` is `[LogicPrimitive; 13]`, so an
  exhaustive match on either enum no longer compiles. `LogicPrimitive::call_template` is
  `parenthesised_template`, which also writes an applied primitive, as `delay(…, N)`. `Expr` gains
  `Apply`, which `cairn parse --format json` prints as `{"kind":"Apply",…}`.
