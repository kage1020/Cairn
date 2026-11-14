- *(wasm)* `cairn-lang-wasm` exports the compiler to JavaScript, with the built-in registry packs
  embedded. `compile(source, edition, target)` answers what `cairn compile` would: its findings in
  the `--format json` shape, the run-level refusals and the warnings that name no span, the
  block-array IR, and each structure file's bytes. `info(source, editions)` answers the document
  `cairn info --format json` prints, for a source with no lock beside it, `targets(edition)` the
  versions an edition builds for, and `cairn_version()` the release. This JavaScript API is
  Evolving.
- The documentation site's playground compiles a source in the browser as it is typed. It colours
  the source with the VS Code extension's grammar, underlines and lists each finding, draws every
  structure layer by layer, and offers its `.nbt` or `.mcstructure` to download.
- *(formats)* The `build` module holds what a front end runs over a source for a build: the gate
  `cairn check` applies, the lowering at a target with each struct's circuit laid, the floors the
  registry contradicts, and the run-level refusals. The `info` report is built on it, and the
  `target` module resolves a `--target` to the backend that writes it. `cairn`, the language
  server and the playground all call them, so they report the same findings in the same words.
