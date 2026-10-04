- *(core)* Every command that lowers a source panicked on a scope that painted more than 65,535
  distinct block states besides air: `cairn compile`, `cairn lower`, `cairn info`, and `cairn check
  --edition E --target V`. A source gets there through block ids or state-literal properties no
  pinned target checks. An id goes unchecked wherever no version is pinned, and a state literal's
  properties under every target, so `cairn compile` with a target that resolves panicked too. Every
  paint counts, including one a later member covers. The scope is now skipped with a new warning,
  `W_PALETTE_TOO_LARGE`, and the other scopes still lower; `cairn compile` and `cairn check
  --edition E --target V` then refuse the partial build with `E_PARTIAL_BUILD`, as they do for
  `W_STRUCTURE_TOO_LARGE`. `Palette::try_intern` is the fallible form of `Palette::intern`, its
  `PaletteFull` error carries the capacity it refused at, and `PALETTE_CAPACITY` is the cap.
