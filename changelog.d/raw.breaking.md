- *(core)* `raw_block` and `raw_fill` are no longer `E_UNKNOWN_KEYWORD`, and `key=namespace:path`
  parses as a block id where it was `E_PARSE`.
- *(core)* `MemberRole` gains `RawBlock` and `RawFill`, so an exhaustive match on it no longer
  compiles.
