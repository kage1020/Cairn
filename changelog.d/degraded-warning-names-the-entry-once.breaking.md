- *(formats)* `bedrock_structure::ParityNote` is retyped from `{ id, message }` to
  `{ id, states, dropped }`, so a consumer learns which entry degraded and what it lost without
  parsing English, as it already does from `cairn info`'s `degraded_entries`. `states` is the
  entry's `key=value` pairs, comma-joined, spelled as `DegradedEntry::states` spells them, and
  `dropped` is the `DroppedIntent`; there is still one note per intent dropped. Code that printed
  `message` words the note with `bedrock_state::degradation_detail(&note.dropped)`. `ParityNote`
  is now `#[non_exhaustive]`, so outside the crate it can be read but not built with a struct
  literal.
- *(formats)* `bedrock_state::degradation_message` is removed. The CLI names the entry itself, as
  `id[states]`, ahead of `degradation_detail`'s sentence.
