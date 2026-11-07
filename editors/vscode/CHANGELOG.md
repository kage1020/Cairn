# Changelog — Cairn VS Code extension

Extension versions track the Cairn CLI CalVer tag (`YYYY.M.PATCH`); the release pipeline realigns `package.json` with the workspace on every bump, whether or not a `.vsix` goes out.

Each release attaches the `.vsix` to its GitHub release and publishes the same file to the Visual Studio Marketplace and Open VSX. Until the first such release, everything below is unreleased, and the version in the manifest is the tag it will first ship against.

## [Unreleased]

### Added

- Initial release. Spawns `cairn-lsp` over stdio, wires up the LSP client for `.crn` documents, resolves the server binary from `cairn.serverPath` or `PATH`, and ships a minimal TextMate grammar (comments, directives, top-level and member keywords, material tokens, attribute keys, arrows, strings, numbers). Requires VS Code 1.91 or newer. See the workspace root `CHANGELOG.md` for the full entry alongside the corresponding LSP-side changes.
