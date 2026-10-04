- *(lsp)* A `didChange` event carrying a `range` was stored as the whole document, so `# note\n`
  inserted at `0:0` replaced the file and the server diagnosed and completed text nobody had. The
  server advertises full sync and a conforming client sends no range, but one that does now has
  each event applied in order, ranged or not (`DocumentStore::apply`). A range on a line the
  document does not have, or a range whose end resolves before its start, drops the revision with
  a line on stderr and keeps the last text. A column past its line's end resolves to the line end
  before the two ends are compared.
