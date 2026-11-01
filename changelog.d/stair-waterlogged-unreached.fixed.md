- *(core)* `waterlogged=` on a `stair` refused the build with `E_UNKNOWN_ARGUMENT`, which
  `spec/lint` "Error vs warning" keeps for a key outside the keyword's vocabulary, though
  `spec/blockstate` "`waterlogged`" specifies the key and only the compiler does not read it yet. It
  is now an unreached key: `W_IGNORED_ARGUMENT` names it, and the stair is built as if the key were
  not written.
