- *(core)* `xor`, `nand`, `nor` and `mux` are library words, so a `logic` operand that is one of
  them bare, or a dotted name that starts with one, such as `nor.x`, no longer parses. A dotted name
  may still end in one, as in `sig.nor`.
- *(core)* `LogicPrimitive` gains `Xor`, `Nand`, `Nor` and `Mux`, `Notation` gains `Call`, holding
  the new `CallSignature`, and `LogicPrimitive::ALL` is a `&'static [LogicPrimitive]`, so an
  exhaustive match on either enum, or code typed against the 3-element array, no longer compiles.
  `and` moves from rank 1 to rank 2, and `Expr` gains `Call`, holding `NamedOperand`s, which
  `cairn parse --format json` prints as `{"kind":"Call",…}`.
- *(redstone)* The placeholder Edition Cells `JavaXorUnpinned`, `BedrockXorUnpinned`,
  `JavaNandUnpinned`, `BedrockNandUnpinned`, `JavaNorUnpinned`, `BedrockNorUnpinned`,
  `JavaMuxUnpinned` and `BedrockMuxUnpinned` are `JavaTorchXor`, `BedrockTorchXor`,
  `JavaTorchNand`, `BedrockTorchNand`, `JavaTorchNor`, `BedrockTorchNor`, `JavaTorchMux` and
  `BedrockTorchMux`, serialised under those names in snake case, with pinned port delays in place
  of the placeholder 3 ticks and `regenerates` true.
