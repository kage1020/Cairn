- *(lsp)* `DocumentStore::change` is removed. It stored a `didChange` revision's text as the whole
  document, and the server no longer calls it. `DocumentStore::apply` replaces it: it applies each
  event of the notification's `contentChanges` and returns `Result<&str, store::ChangeRefused>`,
  with `ChangeRefused::DocumentNotOpen` where `change` returned `None`.
