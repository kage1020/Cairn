- *(formats)* `blocks.json` is schema 2: `BlocksBase::blocks` and `BlocksDiff::added` are
  `BlockEntries`, a list of IDs or a map from each ID to its `BlockStates`, rather than
  `Vec<String>`, and `BlocksDiff` gains `changed`. `BlocksIndex::from_catalog` takes the pack's
  `Edition`. An external pack's `blocks.json` is refused when:
  - its tables are not listed oldest first (`BlocksOutOfOrder`), or a diff does not inherit the
    table listed just before it;
  - it holds a field the schema does not name, or names an ID or a state property twice;
  - a schema 1 table gives states or a `changed`, or a schema 2 table lists bare IDs. A schema 1
    file still loads, with no states;
  - a diff removes an ID twice, or removes it and also changes it;
  - a property takes no value, values of two kinds or one value twice, or, on Java, a value that
    is not a string.
- *(formats)* `data_versions.json` is schema 3: a Bedrock row's `data_version` is the release's own
  number packed as `major << 24 | minor << 16 | patch << 8`, an ordering key, and the block-state
  version its structures carry moves to the new `DataVersionEntry::block_version`, since 1.21.60
  and 1.26.60 carry the same one. `BedrockTarget` gains `order_key`, the key an `@requires` floor
  is weighed against.
- *(cli)* `--target latest` builds for Java 26.3 and Bedrock 1.26.60, so a lock written for
  `latest` before reports `W_PREVIOUSLY_VERIFIED_TARGET` on its next build.
