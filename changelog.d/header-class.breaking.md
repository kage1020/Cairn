- *(core)* A `struct` or `def` header takes `size=` and nothing else. A header `class=` was accepted
  and reported as `W_IGNORED_ARGUMENT`, since no pass read it and no theme selector could reach it;
  it is now `E_UNKNOWN_ARGUMENT`, and the build stops. Remove it from the header, or move it to the
  member lines it was meant to group. A label key on a header, such as `class=1`, is that one
  `E_UNKNOWN_ARGUMENT` rather than an `E_TYPE_MISMATCH_LABEL` beside it.
