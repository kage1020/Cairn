- *(lsp)* `cairn-lsp` runs `cairn.diffBlocks` through `workspace/executeCommand`, so a lift can be
  compared with its structure file while it is edited. Given an open document and a structure
  file's `file:` URI, it returns the JSON `cairn diff-blocks --format json` prints for the two: the
  report, or the `{"diagnostics": [...]}` document of a refused run. The source is the text the
  editor holds, saved or not.
- *(vscode)* **Cairn: Diff Blocks** asks for a structure file, then asks the server for that report
  on the active `.crn` file and opens it beside the editor.
- *(formats)* `diff_blocks::diff_blocks` builds a lift at its structure file's stamp and compares
  the two, once for the CLI and the language server, on `build::lower_for_build`, which lowers a
  source for one edition's build as `cairn compile` does.
