# Changelog — Cairn VS Code extension

Extension versions track the Cairn CLI CalVer tag (`YYYY.M.PATCH`). From 2026.12.0, every Cairn release ships one: the `.vsix` is attached to the GitHub release, and the same file is published to the Visual Studio Marketplace and Open VSX.

## [Unreleased]

### Added

- Initial release. Spawns `cairn-lsp` over stdio, wires up the LSP client for `.crn` documents, resolves the server binary from `cairn.serverPath` or `PATH`, and ships a minimal TextMate grammar (comments, directives, `@edition` guards, top-level and member keywords, material tokens, block ids, attribute keys, arrows, strings, coordinate triples, numbers). Requires VS Code 1.91 or newer. See the workspace root `CHANGELOG.md` for the full entry alongside the corresponding LSP-side changes.
- `cairn.edition` and `cairn.target` settings, which `cairn-lsp` reads as the edition and target to check diagnostics against, and the `cairn.setTarget` command, which the quick-fix for `E_VERSION_CAP` runs to set `cairn.target` to the lowest target a file's floors admit. The extension declares the command to the server under `experimental.commands` at `initialize`.
