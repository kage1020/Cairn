- *(formats)* `blocks.json` is schema 2: `BlocksBase::blocks` and `BlocksDiff::added` are
  `BlockEntries`, a list of IDs or a map from each ID to its `BlockStates`, rather than
  `Vec<String>`, and `BlocksDiff` gains `changed`. `data_versions.json` is schema 3: a Bedrock row's
  `data_version` is the release's own number packed as `major << 24 | minor << 16 | patch << 8`, an
  ordering key, and the block-state version its structures carry moves to the new
  `DataVersionEntry::block_version`, since 1.21.60 and 1.26.60 carry the same one.
  `BedrockTarget` gains `data_version`, `PackManifest` gains `extracted`, and `RegistryError` and
  `BlocksError` gain the variants the new checks refuse with. An external pack whose `blocks.json`
  is schema 1 still loads, with no states; a Bedrock table whose targetable row gives no
  `block_version` is refused.
- *(cli)* `--target latest` builds for Java 26.3 and Bedrock 1.26.60, so a lock written for
  `latest` before reports `W_PREVIOUSLY_VERIFIED_TARGET` on its next build.
