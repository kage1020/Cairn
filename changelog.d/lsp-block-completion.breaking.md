- *(lsp)* The `textDocument/completion` result is a `CompletionList` rather than a
  `CompletionItem[]`. Both are valid LSP answers, but a client that reads the result as an array
  breaks. A material-token answer holds only the tokens matching what is typed, at most 100,
  marked `isIncomplete` when it is cut, where it used to hold every token. The canonical ids it
  offers no longer carry the detail `canonical block id`, and an abstract token's detail names what
  it resolves to in each edition rather than Java's default id alone.
- *(lsp)* `cairn_lang_lsp::completion::completions` returns `Option<lsp_types::CompletionList>`
  rather than `Option<Vec<lsp_types::CompletionItem>>`, and a material-token answer is the same
  filtered one, cut at `completion::MATERIAL_ITEM_LIMIT`, where it used to be the whole set.
