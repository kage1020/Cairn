- *(core)* A `door` can name its column by number, `offset=N`, counted from 0 at its wall's left
  end as seen from outside, as a window's `offset=` is. It takes a column between the wall's
  corners, from 1 to the wall's length less 2. A corner column, which `at=left` and `at=right`
  name, or one at or past the wall's end, is `E_INVALID_MEMBER`, with a note naming the columns
  `offset=` takes. A door names its column once, so `at=` beside `offset=` is refused, and `at=3`
  is told to write `offset=3`. A walkway port on such a door reads the column the door stands in.
  Every refusal of a door's column now gives its fix in a note rather than in the message, and a
  door with neither key is told it has no `at=` or `offset=`.
