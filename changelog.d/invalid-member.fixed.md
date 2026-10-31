- *(core)* A member dropped for a fault in its own line was only warned about, so a structure built
  at exit 0 without the window, door, roof or stair its author wrote, which `spec/lint` "Error vs
  warning" forbids for a dropped member. Such a member is now `E_INVALID_MEMBER`, the code a
  pressure plate its line puts nowhere already earns. `W_DEFERRED_MEMBER` keeps three cases: a
  member another member keeps out, such as a `door` or `window` in a course whose `walls` paint
  nothing or an eave `stair` beside a `roof` whose `overhang=` draws nothing, with a note naming
  that member; a line the pass does not lower yet, such as a `floor` or `roof` under a raised
  `level`; and part of a built member, such as an overlapping `sym=true` mirror. A line wrong in
  itself is the error even when such a member is missing too. A `walls` with no `height=`, a
  `level` with no `y=` and a `stair` with no `kind=` now say what to write, and a `door` or
  `window` outside every course names the courses the walls are written to.
