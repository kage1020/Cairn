- *(core)* `suggest::did_you_mean`, the one phrasing every "did you mean" note, message and error
  uses.
- *(core)* `BlockState::properties_text`, the `key=value,...` spelling of a state's properties.
- *(core)* `lines::LineStarts::line_of`, the 0-based line an offset is on.
- *(core)* `ast::ItemKind::ALL`, every top-level item kind.
- *(formats)* `RegistryPack::targetable`, the rows `--target` may name, and
  `RegistryPack::version_order`, the order `@requires` floors are placed in.
- *(redstone)* `PlacementStage` and `RouteLayer` derive `Serialize`, and the four `Scoped*` IR
  lists are aliases of one `Scoped<T>` / `ScopedEntry<T>` pair with the same fields and JSON.
