- *(core)* A `mat_slot=` on a `door`, `level`, `circuit` or `place` changes nothing in the build,
  since none of them reads a material, yet a slot name the bound theme lacked refused the build
  with `E_UNRESOLVED_SLOT` (on a `place` it went unreported). The key is now `W_IGNORED_ARGUMENT`
  on its value, as any other key no pass reads is, and is not looked up in the theme; removing it
  leaves the build as it was. Nor does it count as reading a slot, so a module whose only
  `mat_slot=` is on one of the four is not refused with `E_THEME_VARIANT_MISSING` when the pinned
  `--edition` can bind none of its theme's variants.
