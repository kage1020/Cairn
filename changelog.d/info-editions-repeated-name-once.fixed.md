- *(cli)* `cairn info --editions java,java` reported Java twice: a second entry in
  `edition portability` and `buildable targets`, in text and in the JSON document, and every
  per-edition note on stderr printed a second time, since the per-edition dry-run ran once per
  name. That run now walks the deduplicated list the `@intended_targets` findings are weighed in,
  so a repeated edition is reported once.
