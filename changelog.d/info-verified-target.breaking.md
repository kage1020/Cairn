- *(cli)* `cairn info` prints six rows where it printed five, the new `verified target` last, so a
  pipeline that counts the rows reads one more, and one that takes the last as `semantic-sensitive`
  now gets `verified target`. A successful run also writes a `note:` to stderr when the lock is
  missing, unreadable, of a newer schema, records a refused identifier, or was written for another
  revision of the source.
- *(core)* `Lockfile::refused_identifier` is removed: `lock::LockfileRead::from_body` reads a
  lock's text as valid, refused only for an identifier, of a newer schema, or invalid, and names
  what else is wrong with a lock before its refused identifier. `RefusedIdentifierLock` gains
  `source_hash`, `cairn_version` and `verified`.
