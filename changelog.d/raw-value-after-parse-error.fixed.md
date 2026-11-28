- *(core)* When a file failed to parse, a character the lexer has no token for in a later
  `@cairn`, `@requires` or part `requires` value was reported as the build's `E_PARSE`, hiding the
  real failure. Those values are raw text wherever the parse stopped, so the parse error is the one
  reported.
