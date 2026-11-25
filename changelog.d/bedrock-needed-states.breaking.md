- *(core)* `transliterate::GridEntry` gains `needed`, the properties a line writes even at their
  default because a line that leaves one out is not built as the block, so a struct literal of it
  no longer compiles. A needed property `unread` also names is compared only by whether a line
  writes it.
- *(formats)* `registry::CanonicalSpelling` gains `needed`, which `VersionStates::canonical_spelling`
  fills with the fewest such properties, so a struct literal of it no longer compiles. Its `unread`
  keeps a needed property Bedrock does not read.
