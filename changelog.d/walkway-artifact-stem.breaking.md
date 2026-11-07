- *(core,formats)* A walkway's file name is built from the walkway's typed site, places and
  ports instead of by parsing its `walkway::` scope key back apart:
  - `cairn_lang_formats::output_filename` takes the file stem rather than the scope key. Pass it
    `BlockArrayIr::artifact_stem(scope)`.
  - `cairn_lang_core::artifact_stem` names `struct::` and `site::` scopes only, and returns a
    `walkway::` key unchanged. A walkway's stem is `Walkway::artifact_stem`, or
    `walkway_artifact_stem(site, from, to)` from its parts.
