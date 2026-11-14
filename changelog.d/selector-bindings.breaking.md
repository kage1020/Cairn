- *(core)* A `theme` selector row's binding is no longer `W_IGNORED_ARGUMENT`. A key its keyword
  binds (`trim=` on `walls`, `frame=` on `door` and `window`) is built, so a source with such rows
  builds more blocks, its palette and `resolved_ir_hash` move, and a value the target cannot build
  refuses the build where it used to warn. Any other key is `E_UNKNOWN_ARGUMENT`, an error naming
  the keys the keyword binds, where it used to be a warning.
