- *(core)* `waterlogged=` on a `stair` refused the build with `E_UNKNOWN_ARGUMENT`, which
  `spec/lint` "Error vs warning" keeps for a key outside the keyword's vocabulary, though
  `spec/blockstate` "`waterlogged`" defines the key and what is missing is a pass that reads it.
  The key is now `W_IGNORED_ARGUMENT` on its value, as any other key no pass reads yet is, and the
  stair is built as if the key were not written.
- *(spec)* `spec/blockstate` "`waterlogged`" now gives `W_IGNORED_ARGUMENT` for that key, and
  says that a `roof` or a `window` takes no state override, so `waterlogged=` there is still
  `E_UNKNOWN_ARGUMENT`. `spec/lint` "Error vs warning" lists the key among those no pass reads yet.
