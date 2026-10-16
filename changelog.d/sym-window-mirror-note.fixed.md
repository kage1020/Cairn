- *(core)* The `W_DEFERRED_MEMBER` on a `sym=true` window whose mirror overlaps or coincides with
  it carried the note every deferral gets, "block-array lowering currently voxelises floor, walls,
  door, window, …", though a `window` is lowered and this one was cut. The note now says when a
  window and its mirror make a pair, with the bounds for that wall, and how to fix it: move or
  narrow the window, or drop `sym=true` to keep the one opening that was cut.
- *(spec)* `spec/syntax` "Selectors" now says that a mirror which only touches its window is not
  rejected: both are cut, and read as one opening twice the window's width.
  `spec/components-editing-sites` "Ports and `connect`" now says that a `sym=true` window whose
  mirror is skipped keeps its own cut and its port, as lowering already did.
