- *(lsp)* `cairn-lsp` runs `cairn.info` through `workspace/executeCommand`, so an editor can show
  a file's version range, edition portability and buildable targets without a terminal. For an
  open `file:` document, with an optional `editions` list, it returns the JSON
  `cairn info --format json` prints for that file: the report, or the `{"diagnostics": [...]}`
  document of a refused run. The source is the text the editor holds, saved or not, and the lock
  is the `<source>.lock` beside it.
- *(vscode)* **Cairn: Show Info** asks the server for that report on the active `.crn` file and
  opens it beside the editor.
- *(formats)* `info::info` builds the `cairn info` report, its rows, the `note:` lines behind them
  and its JSON document, once for the CLI and the language server, which reports its stderr side
  through an `InfoSink`.
