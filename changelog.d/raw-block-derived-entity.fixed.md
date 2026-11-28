- *(formats)* On Bedrock, a `raw_block` chest half that pairs with its other half, or a note block
  off its default note, lost the compound its `nbt=` wrote: the writer built the pairing or the
  note from scratch over it, so a double chest's `Items` and a note block's `CustomName` were
  dropped without a diagnostic. The writer now puts the compound over what it fills, key by key,
  as it does over an entity's: a key the compound names stays as written and the writer adds the
  others.
