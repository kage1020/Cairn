- *(core)* A member dropped for a fault in its own line was only warned about, so a structure built
  at exit 0 without the window, door, roof or stair its author wrote, which `spec/lint` "Error vs
  warning" forbids for a dropped member. Such a member is now `E_INVALID_MEMBER`, as a pressure
  plate its line puts nowhere is. Its line is judged against the struct as written, every member at
  the size its line asks for whether or not it builds, so a roof that draws nothing still counts for
  the rows it rises and the overhang it adds. `W_DEFERRED_MEMBER` keeps three cases: a member
  another member keeps out, such as a `door` or `window` in a course whose `walls` paint nothing or
  beside walls under a `level` that is not placed, or an eave `stair` beside a `roof` that draws
  nothing, with a note saying which member has the repair; a line the pass does not lower yet, such
  as a `floor` or `roof` under a raised `level`; and part of a built member, such as an overlapping
  `sym=true` mirror. A line wrong in itself is the error even when such a member is missing too or
  its line is one the pass does not lower yet, so a `roof` under a raised `level` is refused for its
  `kind=` before it is deferred, and a `window` series for running past its wall before its
  `sym=true` mirror is. A `walls` refused for its `height=`, a `level` for its `y=`, a `stair` with
  no `kind=` and a `window` refused for its own arguments now say what to write, a `door` or
  `window` outside every course names the courses the walls are written to, and a `level` with
  members under it and no `y=`, or a `level` inside another, is reported once, as
  `E_UNSUPPORTED_NESTING`.
