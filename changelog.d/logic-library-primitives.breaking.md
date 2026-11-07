- *(core)* `xor`, `nand`, `nor` and `mux` are library words, so a source whose `logic` operand is a
  signal named by one of them, such as `nor.x`, or the bare word, no longer parses. A dotted name
  may still end in one, as in `sig.nor`.
- *(core)* `LogicPrimitive` gains `Xor`, `Nand`, `Nor` and `Mux`, `Notation` gains `Call`, and
  `LogicPrimitive::ALL` is `[LogicPrimitive; 7]`, so an exhaustive match on either enum no longer
  compiles. `and` moves from rank 1 to rank 2, and `Expr` gains `Call`, holding `NamedOperand`s,
  which `cairn parse --format json` prints as `{"kind":"Call",…}`.
- *(redstone)* The placeholder Edition Cells `JavaXorUnpinned`, `BedrockXorUnpinned`,
  `JavaNandUnpinned`, `BedrockNandUnpinned`, `JavaNorUnpinned`, `BedrockNorUnpinned`,
  `JavaMuxUnpinned` and `BedrockMuxUnpinned` are `JavaTorchXor`, `BedrockTorchXor`,
  `JavaTorchNand`, `BedrockTorchNand`, `JavaTorchNor`, `BedrockTorchNor`, `JavaTorchMux` and
  `BedrockTorchMux`, serialised under those names in snake case, with pinned port delays in place
  of the placeholder 3 ticks and `regenerates` true.
