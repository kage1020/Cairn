# Cairn — VS Code extension

Language support for [Cairn](https://github.com/kage1020/Cairn) (`.crn`) inside VS Code:
push diagnostics, closed-vocabulary completion, and TextMate syntax highlighting. The extension
is a thin client — all analysis runs in the `cairn-lsp` binary shipped alongside the `cairn`
CLI in the Cairn release archive.

## Install

1. Install the extension (`.vsix`): download from the release page or run
   `code --install-extension cairn-lang-<version>.vsix`.
2. Install the `cairn-lsp` binary:
   - Download the archive matching your platform from the
     [Cairn releases](https://github.com/kage1020/Cairn/releases) (each archive contains both
     `cairn` and `cairn-lsp`).
   - Extract and put the binary on your `PATH`, **or** set `cairn.serverPath` to its absolute
     path in your VS Code settings.
3. Open any `.crn` file — the extension activates automatically.

## Settings

| Setting              | Type   | Default | Purpose                                                                                       |
| -------------------- | ------ | ------- | --------------------------------------------------------------------------------------------- |
| `cairn.serverPath`   | string | `""`    | Absolute path to `cairn-lsp`. Empty → look up on `PATH`.                                      |
| `cairn.trace.server` | enum   | `off`   | Trace LSP messages in the Output panel (`off` / `messages` / `verbose`) for bug reports.      |

## Development

Requires Node.js ≥ 20 and `pnpm`.

```sh
pnpm install
pnpm build            # tsc → dist/
pnpm watch            # rebuild on save while developing
pnpm package          # produces cairn-lang-<version>.vsix
```

Sideload for local testing with `code --install-extension ./cairn-lang-<version>.vsix`. Use the
`Cairn Language Server` output channel (View → Output → Cairn Language Server) to inspect the
LSP wire log; set `cairn.trace.server` to `verbose` for full request/response bodies.

## Version compatibility

The extension is released in lock-step with the Cairn CLI: the `cairn-lsp` binary and the
extension version share the same CalVer tag (`YYYY.M.PATCH`). Mixing versions across a major
release boundary is not supported — the extension logs the server version at activation so
mismatches show up in the Output panel.
