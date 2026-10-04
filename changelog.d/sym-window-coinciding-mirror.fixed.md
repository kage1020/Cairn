- *(core)* A `sym=true` window centred on its wall (`2 * offset + size_w = wall_length`) has a
  mirror that is the same rectangle as the window. Lowering painted the window once and reported
  nothing, though `spec/syntax` "Selectors" rejects every mirror that overlaps its window with
  `W_DEFERRED_MEMBER`: the author asked for two windows and got one without being told. The
  centred window now gets that warning too, saying it "coincides with its mirror" where an
  off-centre one "would overlap" it. The window is still painted and its mirror is not, so the
  blocks written do not change.
