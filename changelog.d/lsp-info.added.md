- *(lsp)* `cairn-lsp` runs `cairn.info` through `workspace/executeCommand`, so an editor can show
  a file's version range, edition portability and buildable targets without a terminal. For an
  open `file:` document, with an optional `editions` list, it returns the JSON
  `cairn info --format json` prints for that file: the report, or the `{"diagnostics": [...]}`
  document of a run a finding refused. A run a palette alone refused, whose document would be
  empty, is answered with a `RequestFailed` error that says why. The source is the text the editor
  holds, saved or not, and the lock is the `<source>.lock` beside it. The language server's
  commands take the tier of the `cairn` flags in `spec/compatibility`.
- *(vscode)* **Cairn: Show Info** asks the server for that report on the active `.crn` file and
  opens it beside the editor.
- *(formats)* `info::info` builds the `cairn info` report, its rows, the `note:` lines behind them
  and its JSON document, once for the CLI and the language server; its stderr side goes to an
  `InfoSink`.
