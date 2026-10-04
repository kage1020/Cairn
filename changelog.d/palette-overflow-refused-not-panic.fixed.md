- *(core)* `cairn lower`, `cairn info`, and a `cairn check --target` naming a version the edition
  does not ship panicked (exit 101) on a scope that painted more than 65,535 distinct block states,
  which made-up block ids reach because no pinned target checks them. Every paint counts, including
  one a later member covers. The scope is now refused with a new warning, `W_PALETTE_TOO_LARGE`, and
  the rest of the build goes on; `cairn compile` refuses the partial build as it does for
  `W_STRUCTURE_TOO_LARGE`. `Palette::try_intern` and `PaletteFull` are the fallible form of
  `Palette::intern`, and `PALETTE_CAPACITY` is the cap.
