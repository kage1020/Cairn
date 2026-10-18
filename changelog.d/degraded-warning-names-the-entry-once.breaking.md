- *(formats)* `bedrock_structure::ParityNote` is retyped from `{ id, message }` to
  `{ id, states, dropped }`, so a consumer learns which entry degraded and what it lost without
  parsing English, as it already does from `cairn info`'s `degraded_entries`. `states` is the
  entry's `key=value` pairs, comma-joined, spelled as core's `resolve::DegradedEntry::states`
  spells them, and `dropped` is the `DroppedIntent`; there is still one note per intent dropped.
  Code that printed `message` names the entry from `id` and `states` and words the loss with
  `bedrock_state::degradation_detail(&note.dropped)`. `ParityNote` is now `#[non_exhaustive]`, so
  outside the crate it can be read but neither built with a struct literal nor destructured
  without `..`.
- *(formats)* `bedrock_state::degradation_message` is removed: name the entry from `ParityNote`'s
  `id` and `states`, then append `degradation_detail`'s sentence.
- *(formats)* `bedrock_state::degradation_detail` no longer names the dropped value. Its sentence
  is worded per `key` (``Bedrock has no `shape` state; …``), and the value is the `DroppedIntent`'s
  to read.
