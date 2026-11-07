- *(core,formats)* A structure's file name comes from the lowered IR, not from its scope key:
  - `cairn_lang_formats::output_filename(&str, OutputExt)` keeps its signature, but its first
    argument is now the file stem rather than the scope key. A call that still passes a scope key
    compiles without a warning and returns `struct::cottage.nbt`; pass it
    `cairn_lang_core::block_array::BlockArrayIr::artifact_stem(scope)` instead.
  - `cairn_lang_core::artifact_stem` is removed. A lowered scope's stem is
    `cairn_lang_core::block_array::BlockArrayIr::artifact_stem`, one walkway's is
    `cairn_lang_core::block_array::Walkway::artifact_stem`, and
    `cairn_lang_core::walkway_artifact_stem(site, from, to)` builds a walkway's from its parts.
