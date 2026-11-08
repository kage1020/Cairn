- *(spec)* The redstone DSL of chapter 14 turns Stable, and with it M6 is reached: `cairn compile`
  builds each struct's `logic` graph into the structure file, and a tick simulator checks
  `redstone-door`'s assertions against the circuit it built. The `logic`, `circuit` and `assert`
  lines, the built-in logic library, signal bindings and the Tier 0 parts now break only after a
  release of `W_DEPRECATED` warning, a release ahead of the rest of `.crn` syntax, which turns
  Stable at M3. The diagnostic codes the redstone passes raise stay Evolving until M3, as every
  code does. Syntax a chapter 14 section specifies and the compiler does not build yet is flagged
  subject to change, as in chapters 5 to 9, and stays Evolving; no section of the chapter needs the
  flag today.
