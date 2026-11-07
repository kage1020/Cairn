- *(core)* A state literal outside its block's states at the pinned target, which a Java build
  wrote as given with `W_STATE_LITERAL_UNCHECKED`, is now `E_STATE_DOMAIN` and refuses the build.
  `W_STATE_LITERAL_UNCHECKED` is raised only at a pinned target whose pack records no states for
  the block, so the built-in packs never raise it, and a lowering with no version pinned, as in
  `cairn lower`, `cairn check` without `--target` and the language server, raises neither.
