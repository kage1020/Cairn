- *(lsp)* `cairn-lsp` answers `textDocument/diagnostic`, so a client that pulls diagnostics, as VS
  Code does, asks for a document's findings when it needs them, and is answered `unchanged` when the
  report it holds is still the one for the document's text. A pull the client cancels before the
  server reaches it is answered `RequestCancelled` and never computed. A client whose `initialize`
  declares pulled diagnostics is no longer pushed `publishDiagnostics`; one that does not declare
  them is pushed them as before.
