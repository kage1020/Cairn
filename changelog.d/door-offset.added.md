- *(core)* A `door` can name its column by number, `offset=N`, counted from 0 at its wall's left
  end as seen from outside, as a window's `offset=` is. It takes a column between the wall's
  corners, from 1 to the wall's length less 2. A corner column, which `at=left` and `at=right`
  name, or one of the wall's length or more, is `E_INVALID_MEMBER`, with a note naming the columns
  `offset=` takes. A door names its column once, so `at=` beside `offset=` is refused. `at=` takes
  an anchor and nothing else, and `at=3` is told to write `offset=3`, or `at=left` or `at=right`
  when the number names a corner. A walkway port on such a door reads the column the door stands
  in.
  Every refusal of a door's column now gives its fix in a note rather than in the message, and a
  door with neither key is told it has no `at=` or `offset=`.
