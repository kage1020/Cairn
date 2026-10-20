- *(core)* A key written twice on the right of one `theme` selector row, as in
  `window[class=small] -> frame=@no_such_block frame=@spruce_wood`, got `E_DUPLICATE_ARG` and a
  `W_IGNORED_ARGUMENT` on its last value only, so the first value went without the finding it gets
  when written alone. Each value now gets its own `W_IGNORED_ARGUMENT`, beside the
  `E_DUPLICATE_ARG`.
- *(core)* `E_DUPLICATE_SELECTOR` stays an error while selector bindings are reserved, and the
  specification's Lint chapter now says why: whatever a binding comes to mean, the later row leaves
  the earlier value dead text, the same fault `E_DUPLICATE_ARG` refuses inside one row. The
  finding's note now calls the earlier value dead text rather than lost.
