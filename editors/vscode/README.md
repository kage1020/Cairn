# Cairn — VS Code extension

Language support for [Cairn](https://github.com/kage1020/Cairn) (`.crn`) inside VS Code: diagnostics as you type, against the edition and target you pin, closed-vocabulary completion, hover, quick-fixes, and TextMate syntax highlighting. The extension is a thin client — all analysis runs in the `cairn-lsp` binary shipped alongside the `cairn` CLI in the Cairn release archive.

## Install

1. Install the extension, from whichever source your editor reads. Every Cairn release from 2026.12.0 on publishes it to all three:
   - VS Code: the [Visual Studio Marketplace](https://marketplace.visualstudio.com/items?itemName=kage1020.cairn-lang). Search the Extensions view for "Cairn", or run `code --install-extension kage1020.cairn-lang`.
   - VSCodium and other VS Code forks: [Open VSX](https://open-vsx.org/extension/kage1020/cairn-lang).
   - Offline, or pinned to one release: every [Cairn release](https://github.com/kage1020/Cairn/releases) attaches `cairn-lang-<version>.vsix` with its SHA-256 and sigstore bundle, the same file the two registries serve. Install it with `code --install-extension ./cairn-lang-<version>.vsix`.
2. Install the `cairn-lsp` binary. The extension carries no server of its own, however it was installed. It starts the binary `cairn.serverPath` names, or, while that setting is empty, the `cairn-lsp` on `PATH`.
   - Download the archive matching your platform from the same [Cairn release](https://github.com/kage1020/Cairn/releases) as the extension. Each archive contains both `cairn` and `cairn-lsp`.
   - Extract it and put the binary on your `PATH`, **or** set `cairn.serverPath` to its absolute path in your VS Code settings.
3. Open any `.crn` file. The extension activates automatically.

## Settings

| Setting              | Type   | Default | Purpose                                                                                                                                                |
| -------------------- | ------ | ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `cairn.serverPath`   | string | `""`    | Absolute path to `cairn-lsp`. Empty → look up on `PATH`. Reload the window after changing.                                                             |
| `cairn.edition`      | enum   | `""`    | Edition diagnostics are checked for (`java` / `bedrock`), as `cairn check --edition` checks it. Empty → no edition.                                    |
| `cairn.target`       | string | `""`    | Version of `cairn.edition` diagnostics are checked against, as `cairn compile --target` checks it, `@requires` floors included. Empty → edition alone. |
| `cairn.trace.server` | enum   | `off`   | Trace LSP messages in the Output panel (`off` / `messages` / `verbose`) for bug reports.                                                               |

A change to `cairn.edition` or `cairn.target` re-checks every open document, with no reload. A setting the server refuses, such as a target the edition does not ship, is shown as an error message. The quick-fix for a target below a file's `@requires` floor (`E_VERSION_CAP`) can set `cairn.target` to the lowest target the floors admit: it writes the setting where `cairn.target` is already set, or else where `cairn.edition` is.

## Development

Requires Node.js ≥ 22 and `pnpm`.

```sh
pnpm install
pnpm build            # esbuild → dist/extension.js
pnpm watch            # rebuild on save while developing
pnpm test             # open a .crn file in VS Code against a stand-in cairn-lsp
pnpm package          # produces cairn-lang-<version>.vsix, as the release does
```

`pnpm test` downloads VS Code into `.vscode-test/`. Its stand-in `cairn-lsp` is a shell script, so the tests are skipped on Windows. `pnpm test` opens a window, so on a headless Linux machine run it as `xvfb-run -a pnpm test`.

Sideload for local testing with `code --install-extension ./cairn-lang-<version>.vsix`. Use the `Cairn Language Server` output channel (View → Output → Cairn Language Server) to inspect the LSP wire log; set `cairn.trace.server` to `verbose` for full request/response bodies.

## Version compatibility

The extension's version follows the Cairn CLI: the release pipeline sets it to the same CalVer tag (`YYYY.M.PATCH`) as the `cairn-lsp` binary. Use the extension with the `cairn-lsp` of the same release. The extension logs the server version at activation, so a mismatch shows up in the Output panel.
