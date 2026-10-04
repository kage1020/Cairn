- *(core,cli)* A lockfile recording an identifier this build refuses, such as one an earlier Cairn
  wrote before the identifier rule tightened, was reported as one that "could not be read", and the
  target it recorded was not compared, so a target change went without
  `W_PREVIOUSLY_VERIFIED_TARGET` or `W_SEMANTIC_SENSITIVITY`. When the document is valid in every
  other respect, `compile` now says it records an identifier this build refuses, and still compares
  the target; one broken in any other way is still reported as unreadable, with that cause.
  `Lockfile::refused_identifier` makes the same reading.
