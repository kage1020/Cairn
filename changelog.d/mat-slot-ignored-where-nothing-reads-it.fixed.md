- *(core)* A `mat_slot=` on a `door`, `level`, `circuit`, `place` or `connect` changes nothing in
  the build, since none of them reads a material from a slot, yet a slot name the bound theme lacked
  refused the build with `E_UNRESOLVED_SLOT` (on a `place` or a `connect` it went unreported). The
  key is now `W_IGNORED_ARGUMENT` on its value, as any other key no pass reads is, and is not looked
  up in the theme, so a misspelt name is not caught there either; removing it leaves the build as
  it was. The note says why nothing reads it: on a `door` the key waits on the door block, which is
  not placed yet; on the other four it has nothing to be read for, so the note asks for its removal.
  Nor does such a `mat_slot=` count as reading a slot: a module whose `struct` and `def` members
  read no other is not refused with `E_THEME_VARIANT_MISSING` when the pinned `--edition` can bind
  none of its theme's variants. A `place` naming such a theme is still refused, whatever its `def`
  reads, since its own `theme=` binds the theme.
- *(lsp)* Completion no longer offers the theme's slot names after `mat_slot=` on those five
  keywords.
