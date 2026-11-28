- *(core)* In a file that failed to parse, a character the lexer has no token for in a later
  `@cairn`, `@requires` or part-level `requires` value took the `E_PARSE` in place of the failure.
  A line the parse never reached is read as raw where one of those values stands, a header for the
  first two and the top of a `def` or `theme` body for the third, so the parse failure is the one
  reported.
