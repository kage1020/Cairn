# Changelog — Cairn VS Code extension

Extension versions track the Cairn CLI CalVer tag (`YYYY.M.PATCH`); the release pipeline
realigns `package.json` with the workspace on every bump, whether or not a `.vsix` goes out.

No version of this extension has been published to the Marketplace yet, so everything below is
still unreleased and the version in the manifest is the tag it will first ship against.

## [Unreleased]

### Added

- Initial release. Spawns `cairn-lsp` over stdio, wires up the LSP client for `.crn`
  documents, resolves the server binary from `cairn.serverPath` or `PATH`, and ships a minimal
  TextMate grammar (comments, directives, top-level and member keywords, material tokens,
  attribute keys, arrows, strings, numbers). See the workspace root `CHANGELOG.md` for the full
  entry alongside the corresponding LSP-side changes.
