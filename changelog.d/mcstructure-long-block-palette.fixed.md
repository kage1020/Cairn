- *(formats)* `prepare_mcstructure` and `build_mcstructure_tag` panicked on a hand-built
  `BlockArray` whose palette translates to more than 65,536 distinct Bedrock blocks. They now write
  it, as the Java writer writes a long palette. Translating the palette also no longer compares
  each entry with every block before it, so preparing a palette that large takes a moment rather
  than minutes. The CLI never builds such a palette.
