- *(core)* `lock::LockEdition` is gone: `LockTarget::edition` is `Edition`, which now derives
  `Deserialize`. The lockfile's YAML is unchanged.
- *(core)* `block_array` no longer exports the walkway router (`BlockedIndex`, `RoutePathError`,
  `WalkwayLayout`, `build_walkway_array`, `l_path`, `route_path`). Lowering is its only caller;
  the walkways it lays are still in `BlockArrayIr::walkways`.
- *(core)* `compute_axes` takes the module and the edition reports only. It never read the Intent IR
  or the resolution it was also given.
- *(formats)* `registry::PackEdition` is gone: `PackManifest::edition`,
  `RegistryError::EditionMismatch` and `UnsupportedTarget::edition` carry core's `Edition`.
- *(formats)* `builtin_java` / `builtin_bedrock` become `builtin_pack(edition)`, and
  `load_builtin_java` / `load_builtin_bedrock` become `load_builtin_pack(edition)`, under
  `registry` and at the crate root.
- *(formats)* `data_version::supported_list` is gone; `RegistryPack::supported_list` is the list
  the CLI prints.
- *(formats)* `bedrock_state::translate_states` takes the `BlockState` rather than its id and
  properties.
- *(lsp)* `LineIndex::line_end` is gone; `cairn_lang_core::lines::LineStarts::line_end` answers the
  same question.
