- *(core)* A `level` whose `y=` is written but is not a row, such as `y=abc`, was told it had no
  `y=`. The message now names the value it read and asks for `y=N` in its place.
- *(core)* A door's `at=` or a port's `side=` written as a number, a string or a coordinate is named
  in the refusal as written. Only a number on `at=` is still told that numeric offsets are
  reserved.
