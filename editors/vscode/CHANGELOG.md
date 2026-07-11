# Changelog — Cairn VS Code extension

Extension versions track the Cairn CLI CalVer tag (`YYYY.M.PATCH`).

## [Unreleased]

### Added

- Initial release. Spawns `cairn-lsp` over stdio, wires up the LSP client for `.crn`
  documents, resolves the server binary from `cairn.serverPath` or `PATH`, and ships a minimal
  TextMate grammar (comments, directives, top-level and member keywords, material tokens,
  attribute keys, arrows, strings, numbers). See the workspace root `CHANGELOG.md` for the full
  entry alongside the corresponding LSP-side changes.
