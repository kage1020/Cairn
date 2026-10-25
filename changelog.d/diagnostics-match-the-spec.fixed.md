- *(core)* A key a `theme` selector coins was refused as `E_UNKNOWN_ARGUMENT` whenever it was near a
  key the role already has, up to the "did you mean" cap, though `spec/lint` refuses only a word
  one edit away. A coined key two or more edits from every real one is now its own key, as the
  spec says; one edit away is still refused.
- *(core)* `E_UNRESOLVED_SLOT` now lists the slots the theme declares, or says it declares none, as
  `spec/lint` asks of every diagnostic. `E_DUPLICATE_ARG`, `E_DUPLICATE_SIZE`, `E_DUPLICATE_ID`,
  `E_TYPE_MISMATCH_SIZE` and `E_TYPE_MISMATCH_LABEL` gain a note saying how to fix them.
- *(cli)* `cairn --help` said the compiler turns `.crn` into NBT "and back", and `--out` and
  `cairn lower` named only Java's `.nbt`. They now say it writes structure files, `.nbt` for Java
  and `.mcstructure` for Bedrock. `--edition`'s values no longer say what each one emits, since
  `check` and `synth` take the same flag and write no file.
