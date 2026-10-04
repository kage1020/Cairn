- *(core)* The "did you mean" search computed the full Damerau-Levenshtein distance between an
  unknown id and every candidate, though no candidate more than three characters longer or shorter
  can pass the cap. One unknown `@token` of 20,000 characters cost `compile --target` about 2 s and
  `info` about 20 s in a release build, for an answer that was always "no suggestion". A candidate
  outside the cap's length band is now skipped before its distance is computed, which changes no
  suggestion.
