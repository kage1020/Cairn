- *(lsp)* `cairn-lsp` runs `cairn.diffBlocks` through `workspace/executeCommand`, so a lift can be
  compared with its structure file while it is edited. Given an open document and a structure
  file's `file:` URI, it returns the JSON `cairn diff-blocks --format json` prints for the two: the
  report, or the `{"diagnostics": [...]}` document of a source refused by its findings. A run
  refused as a whole is answered with a `RequestFailed` error carrying the lines that refuse it.
  The source is the text the editor holds, saved or not.
- *(vscode)* **Cairn: Diff Blocks** asks for a structure file, then asks the server for that report
  on the active `.crn` file and opens it beside the editor. A run the server refuses as a whole,
  for either report command, shows the server's message.
- *(formats)* `diff_blocks::diff_blocks` builds a lift at its structure file's stamp, checks it
  with that stamp's backend as `cairn compile` does, and compares the two, and
  `import::decode_structure_file` reads a structure file from disk as its extension names, both
  shared by the CLI and the language server.
