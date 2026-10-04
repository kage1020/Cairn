- *(core)* A `def` whose only `place use=` row was refused for its id or its origin was also
  reported as `W_UNUSED_DEF`, with advice to remove the def. Every such refusal did this: a missing
  `id=` (`E_INCOMPLETE_PLACE`), one that is not an identifier or string (`id=3`,
  `E_TYPE_MISMATCH_LABEL`), `E_INVALID_PLACE_ID`, `E_DUPLICATE_PLACE_ID`, `E_INVALID_PLACE_ORIGIN`
  (no origin selector, more than one, an `at=` other than `origin`, or an `east_of=` / `north_of=`
  that is not an identifier or string), an `east_of=` / `north_of=` naming no prior place
  (`E_UNRESOLVED_PLACE_REF`), and one naming a row refused for its id (`W_DEFERRED_PLACE`).
  Following the advice brought the def back as `E_UNRESOLVED_PLACE_REF` once the row was fixed. A
  row whose `use=` names a declared def now references it, whatever refuses the row.
