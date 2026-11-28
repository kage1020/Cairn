- *(core)* A `raw_repeat` whose raw members all stand under `@edition` guards is dropped by a build
  that drops them all, rather than refused as empty: unpinned `cairn lower` and `cairn info` build
  a repeat over both editions' guarded lines, and a repeat over Bedrock's lines alone writes nothing
  on Java, as its guarded lines do.
