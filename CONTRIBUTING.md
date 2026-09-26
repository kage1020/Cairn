# Contributing to Cairn

> Language: **English** ([日本語](CONTRIBUTING.ja.md))

Cairn is a working compiler with a normative [specification](https://cairn.kage1020.com/spec/) behind it. Both are still moving, and both take contributions.

## Ways to help

- **Bug reports.** A `.crn` file, the command you ran, and what you expected. A file that compiles to the wrong blocks is as valuable as one that crashes.
- **Examples.** Write real builds and say where the language got awkward, ambiguous, or ran out. This is what drives the vocabulary. Small files exercising one member each live alongside the big ones in [`examples/`](examples/).
- **Code.** Parser, lowering, backends, redstone, LSP, tooling. Pick something off the [roadmap](https://cairn.kage1020.com/roadmap/) or scratch your own itch.
- **Spec edits.** Fix errors, clarify wording, improve examples. Keep each chapter self-contained and cross-link with relative links.
- **Design critique.** Challenge a decision, surface a missing case, propose an alternative. Open an issue pointing at the specific chapter and section.
- **Prior art.** Redstone compilers, schematic formats, voxel and CAD place-and-route, HDL synthesis — pointers are welcome in design discussions.

English is the source of truth for the spec and documentation. Translations are welcome as clearly labelled secondary copies.

## Getting set up

[`rust-toolchain.toml`](rust-toolchain.toml) pins an exact compiler and `rustup` picks it up automatically, so a checkout and a `cargo build` gets you the toolchain CI runs.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` writes nothing. `compile` writes structure files and a lockfile next to the source, so point it at `--out` and `--lock` outside the tree if you don't want build output in `examples/`.

Before opening a PR, run what CI runs — the same four commands on Linux, macOS, and Windows, with `RUSTFLAGS=-D warnings` set for all of them:

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
```

CI also builds the API docs on Linux, twice, with rustdoc's warnings fatal — clippy does not see a doc link to a private item or to a path that no longer resolves. The first run is the public surface and the only one that flags a public doc linking to a private item; the second also covers the docs only a contributor reads:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features --document-private-items
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

The [Developer Guide](https://cairn.kage1020.com/development/) covers the dependency rules between crates in more detail.

## Writing style

The habit is **how** in the code, **what** in the commit message, and the minimum **why** in comments.

- The spec is the source of truth. Use its terminology (`intent_state` / `resolved_state`, `mat_slot`, canonical token) rather than inventing synonyms, and introduce a new term in the chapter it belongs to.
- Design principles are cited as `P1`–`P5`, from [Design Principles](https://cairn.kage1020.com/spec/principles/).
- Error messages take the shape *what is wrong / what would be valid / suggested fix*. That shape is what makes the write-check-fix loop work, so it earns the extra sentence.
- Keep examples concrete and minimal.

### No session-local references

Rust source, spec body, examples, and docs have to read on their own years from now. No issue or PR numbers, no `M3-PR4` coordinates, no bare milestone labels, no "a later PR will…". Replace the coordinate with the fact it stood in for:

> Before: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> After: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

When the deferred thing lands, update the comment in the same PR.

Three surfaces are exempt, because milestone vocabulary is what they are *for*: [CHANGELOG.md](CHANGELOG.md), the [roadmap](https://cairn.kage1020.com/roadmap/), and the milestone columns of the [compatibility](https://cairn.kage1020.com/spec/compatibility/) table. Reviewers can check the rest with:

```sh
rg '\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\bPR[0-9]+\b|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' --glob '!CONTRIBUTING*' \
  --glob '!**/compatibility.md' --glob '!**/roadmap.md' --glob '!target/**'
```

An empty result is the contract.

### Cite the spec by name, not by number

A section number is the same kind of coordinate. `§14.5` is true only while "Place-and-route" happens to be the fifth section of the fourteenth chapter, so inserting a chapter or splitting a section turns every comment that named it into a quiet lie — and the only way to find them is to read the whole source tree. Editing the spec should not cost that. Name the chapter file and, when you mean one section, its title:

> Before: ``/// Stable per `spec/lint.md` §11.3: errors are things the compiler refuses to guess at.``
>
> After: ``/// Stable per `spec/lint` "Error vs warning": errors are things the compiler refuses to guess at.``

Write the chapter as its file stem without the extension (`spec/redstone`, `spec/versioning-editions`) and copy the section title verbatim. Cite the chapter alone when you mean the chapter, and once a comment has given the full citation, later sentences in it should refer back in prose — "that pipeline", "the phase order" — rather than repeat it.

`cargo test --workspace` holds this over `crates/`, `examples/` and `.github/`: no `§` or "section 11.3" in any of them, every `spec/<chapter>` names a chapter that exists, and every quoted title is a real heading in it. Renumbering the spec now touches no Rust at all. Retitling a section fails the test at each citation by file and line, which is the point — a title change is a change of meaning, and the comment that leaned on it deserves a re-read.

The rest of the repository is on you. `website/` is unread because that is where the numbers are defined, and the guide pages beside the spec still link to sections by an anchor that bakes the number into the slug — untangling that belongs with a change to the headings themselves. `editors/`, this file, and the changelog are simply outside the scan.

### Crate READMEs

Each crate's `README.md` is what crates.io shows, so it is the front page for anyone who has never seen the repository. When a stage lands, update that crate's README in the same PR — a stage is not shipped while the front page still says it is coming.

Four crate READMEs keep an inventory the code could be compared against, and every one of them is held to it by a test rather than by review: `cairn-lang-core`'s module table against the crate's `pub mod` declarations, `cairn-lang-cli`'s subcommand table against what `cairn --help` lists, and the `## Public API` tables in `cairn-lang-nbt` and `cairn-lang-formats` against what each crate root re-exports. Add a `pub mod`, a subcommand, or a `pub use` without adding its row and `cargo test --workspace` fails, naming the missing one. Every check runs in both directions, so a row for something that no longer ships fails too.

`## Public API` means the crate root's `pub use` re-exports, and nothing else. An item that is only `pub` inside a module — `registry::AliasCatalog`, anything under `registry::blocks` — is deliberately off the table; it belongs in prose if it needs describing. A row writes one full `module::item` path per code span, and may hold several spans when the items share a sentence. Adding a `## Public API` table to a crate that has none means adding that crate to `TABLED_CRATES` in `readme_public_api_tables.rs`, which the same test will tell you.

What no test reads is the prose. A row's description, a "Status" paragraph, a heading calling a table *planned*, a "Not yet here" list — all of it stayed true only because someone remembered, and for two published crates it went on describing an unimplemented skeleton long after the compiler worked. When you move something out of "not yet", move it out of the README too. Say what ships today; keep what is still ahead in a section of its own, so a reader can tell the two apart without running the code.

## Branches and pull requests

`canary` is the trunk. `main` is the released state: after each publish the pipeline opens a promote-to-main PR from `canary` and turns on auto-merge, so `main` moves one approved merge commit per release and never runs ahead of one.

| Branch | Purpose |
|---|---|
| `canary` | All feature work, fixes, and docs land here. Protected. |
| `main` | Moved only by the pipeline's promote-to-main PR, which a maintainer approves. No direct pushes; contributors never open a PR against it. |
| `<type>/<short-kebab>` | One change, targeting `canary`. Deleted after merge. |
| `release-plz-*` | Opened automatically for monthly minors and patches. |

Name the branch after the Conventional Commits type the work will land under: `feat/parser-lexer`, `fix/wall-corner-shape`, `docs/roadmap-2027`.

**The PR title must be a [Conventional Commits](https://www.conventionalcommits.org/) line.** Squash merge is the only merge mode, so that title becomes the commit on `canary` and is what `release-plz` reads to decide whether a patch release is due. Commits on your own branch are free-form. The scope names the crate or spec area (`feat(core)`, `fix(nbt)`, `docs(spec)`, `build(deps)`).

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

**A breaking change puts `!` before the colon** — after the type, or after the scope when one is written: `feat(core)!: replace lexer`, `fix!: …`. The `!` and the changelog go together: a PR that adds an entry under `## [Unreleased]` → `### Breaking changes` in [CHANGELOG.md](CHANGELOG.md) titles itself with `!`, and a PR titled with `!` adds that entry. The hand-written entry is what [compatibility "How a break is communicated"](https://cairn.kage1020.com/spec/compatibility/) requires and what a reader of `CHANGELOG.md` sees; the `!` is the same fact in the commit `release-plz` parses. A reviewer who sees one without the other asks for the missing half before merging.

The `!` does not choose the version. Semver would read it as a major bump; here the release workflow computes the next `YYYY.M.PATCH` from the date and the existing tags and writes it into `Cargo.toml` before `release-plz` runs, and `release-plz` leaves a version that already differs from the published one as it is. Nor does it decide whether a release is due — the type does, per the table above. What it changes is the generated release notes: `release-plz` prefixes the commit's line with `[**breaking**]`, and `protect_breaking_commits` keeps that line even for a type whose lines are otherwise dropped (`docs!:`, `ci!:`).

Every PR you open targets `canary`; the only PR against `main` is the pipeline's own promote-to-main. One maintainer approval and green CI are required. The release PR follows the same rules — merging it publishes and fast-forwards `main`.

## Revisiting a settled decision

Some decisions are deliberately closed: `key=value` over positional arguments, phase-ordered evaluation, recompile-don't-transcode, fail-loud over silent substitution. To reopen one, file an issue that states the decision and where it lives in the spec, gives the concrete case it fails, proposes an alternative with syntax/IR/message examples, and notes the effect on the [evaluation metrics](https://cairn.kage1020.com/spec/evaluation/).

## Bumping the toolchain

The pin is deliberately not a channel. On `stable`, a Rust release turns every open branch red at once, with findings on files the branch never touched. Pinning does not avoid new lints — it decides that they arrive as a pull request somebody chose to open.

Change `channel`, run the CI commands above (a new compiler can produce a *rustc* or *rustdoc* warning, not just a clippy lint, so a pin bump can turn `Docs` red too), fix what it found in the same PR, and type the commit `ci`. A diff that is only "new compiler, plus the fixes it asked for" is one a reviewer can actually read.

The pin is not the MSRV. `rust-version` in the workspace manifest is the floor a consumer needs to build Cairn, and the pin is always newer. A change reaching for a recently stabilised API is green at the pin and broken at the floor. Clippy sees some of that already — `clippy::incompatible_msrv` reads `rust-version` and refuses a *standard library* item stabilised above it, and `-D warnings` makes that fatal — but it is a lint, so one `#[allow]` silences it, and it says nothing about a **dependency** whose own `rust-version` is above ours, which is a hard cargo error nothing at the pin ever sees. CI's `MSRV` job is what compiles at the floor and so catches both: it reads `rust-version` back out of the manifest with `cargo metadata` — no second copy to go stale — installs that compiler, and runs `cargo check --workspace --locked --all-features` at it. `check` rather than `test`, because the floor is about compiling the crates a consumer depends on and dev-dependencies are free to want a newer compiler than the library does; `--all-features`, because `rust-version` is one declaration per package and cargo has no way to say "this floor, unless you enable that feature"; `--locked`, because the committed lockfile is what a consumer cloning the repo resolves to, and when the failure is a dependency's own floor rather than Cairn's code cargo names the package.

Raising `rust-version` changes who can build the crates, so it is not a quiet manifest edit. Type the commit `build` — which cuts a patch release, so the new floor reaches crates.io — and add a `CHANGELOG.md` entry saying which compiler is now required and what needed it. A consumer pinned below the new floor learns about it from cargo either way; the entry is what tells them why.

## Changing the release profile

`[profile.release]` in the workspace manifest is sized for the release archives, and each setting in it records why it is there. Most of them cost only build time. `opt-level` is the exception: it trades throughput in block-array lowering and in place-and-route, which is the work a build spends its time on. Two benches measure that side, each over a source generated large enough that the passes outweigh process startup: `lowering` in `cairn-lang-core` and `place_and_route` in `cairn-lang-redstone`. Benches inherit the release profile, so what they time is the code that ships. Save a baseline, change the profile — or override one setting from the environment without editing the file — and compare:

```sh
cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --save-baseline before
CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --baseline before
```

Name the two benches: without `--bench`, `cargo bench` also runs each library's unit-test harness, which refuses criterion's `--save-baseline`. Run a baseline against itself once before trusting a difference. On a shared machine the smaller benches move by several percent between identical builds.

Weigh a size change on the gzipped binaries, since the release archives are `.tar.gz` and `.zip`, and put the numbers in the commit message rather than the profile comment, where they would go stale on the next dependency bump. CI runs neither bench. A timing gate on shared runners would be noise, and a profile change is a decision made once rather than a regression surface. What CI does keep is that both benches compile, through `clippy --all-targets`. `cargo test -p cairn-lang-core --bench lowering` (and the same for `place_and_route`) runs each benchmark once as a check. Each bench checks that its generated source comes through every pass without losing a scope, so a generator that stopped reaching the passes fails there rather than quietly timing less work.

## Versioning

Date-based, `YYYY.M[.PATCH]`. Notable changes go in [CHANGELOG.md](CHANGELOG.md). What a bump is allowed to break is set by [Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/), not by the number itself.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.
