- *(core)* A key a `theme` selector coins was refused as `E_UNKNOWN_ARGUMENT` whenever it was near a
  key the role already has, up to the "did you mean" cap, though `spec/lint` refuses only a word
  one edit away. A coined key two or more edits from every real one is now its own key, as the
  spec says; one edit away is still refused.
- *(cli)* `cairn info` listed an edition's versions as buildable when the edition's writer could
  not carry a palette entry's states, though `cairn compile` refuses every one of them. The
  `buildable targets` row now refuses them too, prints a `note:` naming each entry, and under
  `--format json` names the entries in `reason.states_unmapped`.
- *(core)* `E_UNRESOLVED_SLOT` now lists the slots the theme declares, or says it declares none, as
  `spec/lint` asks of every diagnostic. Where an unpinned build also takes a sibling theme
  variant's slots, it lists and suggests those as well, and it offers to correct the slot name only
  when there is a slot to correct it to.
- *(core)* `E_DUPLICATE_ARG`, `E_DUPLICATE_SIZE`, `E_DUPLICATE_ID`, `E_DUPLICATE_SLOT`,
  `E_TYPE_MISMATCH_SIZE` and `E_TYPE_MISMATCH_LABEL` gain a note saying how to fix them.
  `E_TYPE_MISMATCH_LABEL`'s note fits its key: under `mat_slot=` it says how to bind a block to a
  slot, and under `use=` and `theme=` it asks for a `def` or `theme` the file declares.
- *(core)* An actuator key on a member that does not read it, such as `walls opened_by=sig.power`,
  is `E_UNKNOWN_ARGUMENT` with a note naming the component the key belongs to.
- *(cli)* `cairn --help` said the compiler turns `.crn` into NBT "and back", and `--out` and
  `cairn lower` named only Java's `.nbt`. They now say it writes structure files, `.nbt` for Java
  and `.mcstructure` for Bedrock. `--edition`'s values no longer say what each one emits, since
  `check` and `synth` take the same flag and write no file.
