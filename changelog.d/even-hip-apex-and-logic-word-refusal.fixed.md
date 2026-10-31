- *(core)* A hip roof whose short span is even capped its two-row apex with stairs that all took the
  low slope's facing, which left a 0.5 × 0.5 undercut along one outer face for the band's whole
  length. Each row now faces away from the ridge, as an even-span gable's apex pair does:
  `north` and `south` across an x-ridge, `west` and `east` across a z-ridge. An odd span's apex is
  unchanged.
- *(core)* What follows a complete operand of a `logic` expression, where an infix primitive, `)`
  or the end of the line belongs, is still `E_PARSE` at the same position, and the message now
  names the infix primitives that could stand there and, where it can, why the text cannot:

  ```text
  s.crn:2:23: error[E_PARSE]: expected `and`, `or`, or end of line, got identifier `xor`: the built-in logic library has no primitive `xor`
  ```
