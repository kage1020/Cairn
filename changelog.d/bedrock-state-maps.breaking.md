- *(core)* `DroppedIntent` is a struct of `key`, `value`, `mc_versions` and `reason` rather than an
  enum with one `Shape` variant, and `UnsupportedReason::StatesUnmapped` carries `detail`, the
  reason the newest version's state map gives, in place of `mapped`. In `cairn info --format json`
  a `states_unmapped` entry carries `detail` instead of `mapped`.
- *(formats)* `translate_states`, `prepare_mcstructure` and `build_mcstructure_tag` take the target
  version's state map, a `VersionStates`, and `portability_for_bedrock` the pack's
  `StateMapIndex`, when it has one. `degradation_detail` is removed, since the pack's `sensitivity`
  words a dropped state. `StateTranslation` gains `id` and `waterlogged`, `BedrockStateError`'s
  variants are `Unmapped`, `UnknownProperty` and `UnknownValue`, and `PackFiles` gains `states`
  and `sensitivity`.
- *(formats)* A Bedrock registry pack with block tables must name `states` and `sensitivity`, and
  a Java pack may name neither, so a Bedrock pack `load_from_dir` read before is refused until it
  carries a state map.
- *(formats)* A Bedrock palette entry is written with every Bedrock state its block takes, the
  Java default's for a property the entry leaves out, and a waterlogged one, or one that always
  stands in water, with water in the second layer, so a `.mcstructure` built now differs from one
  built before. The built-in Bedrock pack's hash moves with its two new components.
