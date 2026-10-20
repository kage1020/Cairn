- *(formats)* `prepare_mcstructure` and `build_mcstructure_tag` panicked on a hand-built
  `BlockArray` whose palette translates to more than 65,536 distinct Bedrock blocks, since no
  `block_palette` slot past `u16::MAX` fits. They now refuse it with the new
  `BedrockStructureError::BlockPaletteFull`, which names the palette entry that needed the slot by
  its position and id, as they already refuse a voxel index outside the palette or a grid that
  disagrees with its dims. An entry past that count that merges into an earlier Bedrock block is
  still written. Translating the palette also no longer compares each entry with every block
  before it, so a palette that large is checked in a moment rather than minutes. The CLI never
  builds such a palette.
