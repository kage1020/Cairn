# Contributing to Cairn

> Language: **English** ([日本語](CONTRIBUTING.ja.md))

Cairn is a working compiler with a normative [specification](https://cairn.kage1020.com/spec/)
behind it. Both are still moving, and both take contributions.

## Ways to help

- **Bug reports.** A `.crn` file, the command you ran, and what you expected. A file that compiles
  to the wrong blocks is as valuable as one that crashes.
- **Examples.** Write real builds and say where the language got awkward, ambiguous, or ran out.
  This is what drives the vocabulary. Small files exercising one member each live alongside the big
  ones in [`examples/`](examples/).
- **Code.** Parser, lowering, backends, redstone, LSP, tooling. Pick something off the
  [roadmap](https://cairn.kage1020.com/roadmap/) or scratch your own itch.
- **Spec edits.** Fix errors, clarify wording, improve examples. Keep each chapter self-contained
  and cross-link with relative links.
- **Design critique.** Challenge a decision, surface a missing case, propose an alternative. Open an
  issue pointing at the specific chapter and section.
- **Prior art.** Redstone compilers, schematic formats, voxel and CAD place-and-route, HDL
  synthesis — pointers are welcome in design discussions.

English is the source of truth for the spec and documentation. Translations are welcome as clearly
labelled secondary copies.

## Getting set up

[`rust-toolchain.toml`](rust-toolchain.toml) pins an exact compiler and `rustup` picks it up
automatically, so a checkout and a `cargo build` gets you the toolchain CI runs.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` writes nothing. `compile` writes structure files and a lockfile next to the source, so
point it at `--out` and `--lock` outside the tree if you don't want build output in `examples/`.

Before opening a PR, run what CI runs — the same three commands on Linux, macOS, and Windows,
with `RUSTFLAGS=-D warnings` set for all of them:

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
```

### The repository

| Path | What lives there |
|---|---|
| `crates/cairn-lang-core` | Lexer, parser, Intent IR, check passes, resolution, block-array lowering |
| `crates/cairn-lang-nbt` | NBT codec — Java big-endian, Bedrock little-endian |
| `crates/cairn-lang-formats` | Structure writers (`.nbt`, `.mcstructure`), registry packs, portability |
| `crates/cairn-lang-redstone` | Logic IR, netlist, placement, routing, delay, crossing legalization |
| `crates/cairn-lang-cli` | The `cairn` binary |
| `crates/cairn-lang-lsp` | The `cairn-lsp` language server |
| `crates/cairn-lang-tree-sitter` | tree-sitter grammar, packaged for npm as `tree-sitter-cairn` |
| `crates/cairn-lang-wasm` | Browser bindings for the future playground — a placeholder so far |
| `editors/vscode` | VS Code extension — a thin client over `cairn-lsp` |
| `website` | Astro + Starlight docs site: spec, tutorial, developer guide, in English and 日本語 |
| `examples` | `.crn` sources (lockfiles are generated output and stay untracked) |

The [Developer Guide](https://cairn.kage1020.com/development/) covers the dependency rules between
crates in more detail.

## Writing style

The habit is **how** in the code, **what** in the commit message, and the minimum **why** in
comments.

- The spec is the source of truth. Use its terminology (`intent_state` / `resolved_state`,
  `mat_slot`, canonical token) rather than inventing synonyms, and introduce a new term in the
  chapter it belongs to.
- Design principles are cited as `P1`–`P5`, from
  [Design Principles](https://cairn.kage1020.com/spec/principles/).
- Error messages take the shape *what is wrong / what would be valid / suggested fix*. That shape is
  what makes the write-check-fix loop work, so it earns the extra sentence.
- Keep examples concrete and minimal.

### No session-local references

Rust source, spec body, examples, and docs have to read on their own years from now. No issue or PR
numbers, no `M3-PR4` coordinates, no bare milestone labels, no "a later PR will…". Replace the
coordinate with the fact it stood in for:

> Before: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> After: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

When the deferred thing lands, update the comment in the same PR.

Three surfaces are exempt, because milestone vocabulary is what they are *for*:
[CHANGELOG.md](CHANGELOG.md), the [roadmap](https://cairn.kage1020.com/roadmap/), and the milestone
columns of the [compatibility](https://cairn.kage1020.com/spec/compatibility/) table. Reviewers can
check the rest with:

```sh
rg '\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' --glob '!CONTRIBUTING*' \
  --glob '!**/compatibility.md' --glob '!**/roadmap.md' --glob '!target/**'
```

An empty result is the contract.

## Branches and pull requests

`canary` is the trunk. `main` is the released state, fast-forwarded automatically after each
release, so its history is exactly the list of public releases.

| Branch | Purpose |
|---|---|
| `canary` | All feature work, fixes, and docs land here. Protected. |
| `main` | Updated only by the release pipeline. No direct pushes, no PRs. |
| `<type>/<short-kebab>` | One change, targeting `canary`. Deleted after merge. |
| `release-plz-*` | Opened automatically for monthly minors and patches. |

Name the branch after the Conventional Commits type the work will land under:
`feat/parser-lexer`, `fix/wall-corner-shape`, `docs/roadmap-2027`.

**The PR title must be a [Conventional Commits](https://www.conventionalcommits.org/) line.** Squash
merge is the only merge mode, so that title becomes the commit on `canary` and is what `release-plz`
reads to decide whether a patch release is due. Commits on your own branch are free-form. Add `!`
for a breaking change (`feat(core)!: replace lexer`); the scope names the crate or spec area
(`feat(core)`, `fix(nbt)`, `docs(spec)`, `build(deps)`).

| Type | When | Cuts a patch release? |
|---|---|---|
| `feat` | New feature, public API, subcommand | Yes |
| `fix` | Behaviour brought back in line with the spec | Yes |
| `perf` | Performance | Yes |
| `refactor` | Internal restructuring, no behaviour change | Yes |
| `build` | Build system, packaging, Cargo dependencies | Yes |
| `docs` | Documentation, spec prose, README, examples | No |
| `test` | Test code only | No |
| `ci` | Workflows, release-plz, `rust-toolchain.toml` | No |
| `chore` | Anything that doesn't ship to users | No |
| `style` | Formatting or lint-only changes | No |

Every PR targets `canary`; PRs against `main` are not accepted. One maintainer approval and green CI
are required. The release PR follows the same rules — merging it publishes and fast-forwards `main`.

## Revisiting a settled decision

Some decisions are deliberately closed: `key=value` over positional arguments, phase-ordered
evaluation, recompile-don't-transcode, fail-loud over silent substitution. To reopen one, file an
issue that states the decision and where it lives in the spec, gives the concrete case it fails,
proposes an alternative with syntax/IR/message examples, and notes the effect on the
[evaluation metrics](https://cairn.kage1020.com/spec/evaluation/).

## Bumping the toolchain

The pin is deliberately not a channel. On `stable`, a Rust release turns every open branch red at
once, with findings on files the branch never touched. Pinning does not avoid new lints — it decides
that they arrive as a pull request somebody chose to open.

Change `channel`, run the three CI commands above (a new compiler can produce a *rustc* warning, not
just a clippy lint), fix what it found in the same PR, and type the commit `ci`. A diff that is only
"new compiler, plus the fixes it asked for" is one a reviewer can actually read.

The pin is not the MSRV. `rust-version` in the workspace manifest is the floor a consumer needs to
build Cairn, and it moves only when the code genuinely starts requiring a newer compiler. Nothing in
CI builds at that floor, so a change reaching for a recently stabilised API goes green on the pin
while consumers at the declared floor break. `cargo +<floor> check --workspace` is what catches it,
and is worth running when you touch a new API.

## Versioning

Date-based, `YYYY.M[.PATCH]`. Notable changes go in [CHANGELOG.md](CHANGELOG.md). What a bump is
allowed to break is set by [Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/),
not by the number itself.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you agree to
uphold it.
