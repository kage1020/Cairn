- *(core)* A key written twice on the right of one `theme` selector row, as in
  `window[class=small] -> frame=@no_such_block frame=@spruce_wood`, got `E_DUPLICATE_ARG` and a
  `W_IGNORED_ARGUMENT` on its last value only, so the first value went without the finding it gets
  when written alone. Each value now gets its own `W_IGNORED_ARGUMENT`, beside the
  `E_DUPLICATE_ARG`.
- *(core)* `E_DUPLICATE_SELECTOR`'s note now calls the earlier value dead text whatever a binding
  comes to mean, rather than lost. The note on the fix said to merge the rows, which writes the key
  twice in one row and so raises `E_DUPLICATE_ARG`; it now says to delete the value you do not mean
  from one of the rows, or to narrow one selector so they pick different members.
- *(spec)* `spec/materials-themes` "Slots as dependency injection" said no selector binding makes
  a build refuse, though a key bound twice for the same members is refused: `E_DUPLICATE_ARG`
  within one row, `E_DUPLICATE_SELECTOR` across two. It now says so, and `spec/lint` "Error vs
  warning" says why `E_DUPLICATE_SELECTOR` is an error while bindings are reserved.
