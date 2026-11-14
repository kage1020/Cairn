- *(lsp)* `cairn_lang_lsp::completion::completions` returns an `lsp_types::CompletionList` rather
  than a `Vec<CompletionItem>`, so a material-token answer can say it was cut at
  `completion::MATERIAL_ITEM_LIMIT`. The canonical ids it offers no longer carry the detail
  `canonical block id`; each names the editions and versions whose blocks tables declare it.
