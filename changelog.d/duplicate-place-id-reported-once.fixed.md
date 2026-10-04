- *(core)* Two `place` rows of one `site` body sharing an `id=` were reported twice, as
  `E_DUPLICATE_PLACE_ID` and as `E_DUPLICATE_ID`, so an editor showed two squiggles on one line
  and a consumer counting errors counted two. `E_DUPLICATE_PLACE_ID`, the code that names the site
  and that the first-row-wins rule is documented for, now reports the pair alone, so the finding
  spans the second row and its note the first, where `E_DUPLICATE_ID` spanned their `id=`. That
  includes two rows sharing an id refused as `E_INVALID_PLACE_ID`, which the resolver did not
  compare: the second row now gets `E_DUPLICATE_PLACE_ID` in place of `E_DUPLICATE_ID`.
  `E_DUPLICATE_ID` still reports a `place` row sharing its `id=` with any other row of the body,
  such as a `connect`, and two `place` rows indented under one row.
