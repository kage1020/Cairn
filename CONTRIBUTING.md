# Contributing to Cairn

> Language: **English** ([日本語](CONTRIBUTING.ja.md))

Cairn is a working compiler backed by a normative [specification](https://cairn.kage1020.com/spec/). Both are still evolving, and both welcome contributions.

## Ways to help

- **Bug reports.** A `.crn` file, the command you ran, and what you expected. A file that compiles to the wrong blocks is as valuable as one that crashes.
- **Examples.** Write real builds and tell us where the language got awkward, ambiguous, or ran out. This feedback is what grows the vocabulary. Small files that exercise one member each sit alongside the big ones in [`examples/`](examples/).
- **Code.** Parser, lowering, backends, redstone, LSP, tooling. Pick something off the [roadmap](https://cairn.kage1020.com/roadmap/) or scratch your own itch.
- **Spec edits.** Fix errors, clarify wording, improve examples. Keep each chapter self-contained and cross-link with root-absolute links (`/spec/syntax/`).
- **Design critique.** Challenge a decision, surface a missing case, propose an alternative. Open an issue pointing at the specific chapter and section.
- **Prior art.** Pointers to redstone compilers, schematic formats, voxel and CAD place-and-route, or HDL synthesis are welcome in design discussions.

English is the source of truth for the spec and documentation. Translations are welcome as clearly labelled secondary copies. Under `website/src/content/docs/`, English pages wrap at 100 columns, and Japanese pages keep each paragraph on one line, because Chromium renders a line break between two Japanese characters as a visible space.

## Getting set up

[`rust-toolchain.toml`](rust-toolchain.toml) pins an exact compiler, and `rustup` picks it up automatically. A checkout and a `cargo build` get you the same toolchain CI runs.

```sh
cargo build --workspace
cargo test --workspace
cargo run -p cairn-lang-cli -- check examples/cottage.crn --edition java --target 1.21.4
```

`check` writes nothing. `compile` writes structure files and a lockfile next to the source; if you don't want build output in `examples/`, point `--out` and `--lock` outside the tree.

Before opening a PR, run what CI runs. CI runs these commands on Linux, macOS, and Windows, all with `RUSTFLAGS=-D warnings`:

```sh
export RUSTFLAGS="-D warnings"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace --locked
cargo test --workspace --locked
cargo test -p cairn-lang-core --bench lowering
cargo test -p cairn-lang-redstone --bench place_and_route
```

`--workspace` does not select bench targets, so the last two run each bench once, as an untimed test. [Changing the release profile](#changing-the-release-profile) explains what they check.

CI also builds the API docs twice on Linux, with rustdoc's warnings fatal, because clippy does not see a doc link to a private item or to a path that no longer resolves. The first run covers the public surface, and it is the only one that flags a public doc linking to a private item. The second also covers the docs only a contributor reads:

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked --all-features --document-private-items
```

CI's `Lint` job checks files against each other rather than testing behaviour: README inventories against the code, spec citations against the spec, the diagnostic catalog against `DiagnosticCode`, and every copy of the version against the workspace manifest. A finding names the file and line. Each rule's module in `xtask/src/rules/` explains why the rule exists:

```sh
cargo xtask lint            # every rule
cargo xtask lint --list     # the rules and what each holds
cargo xtask lint versions   # one rule
cargo xtask lint --site website/dist site-parity   # after `pnpm run build` in website/
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
| `website` | Astro + Starlight docs site: spec, tutorial and guides in English and 日本語, and the developer guide in English. Also the `cairn-lang-docs` crate, which embeds the site's Markdown pages for `cairn docs` |
| `examples` | `.crn` sources (lockfiles are generated output and stay untracked) |

The [Developer Guide](https://cairn.kage1020.com/development/) covers the dependency rules between crates in more detail.

## Writing style

The habit is **how** in the code, **what** in the commit message, and the minimum **why** in comments.

- The spec is the source of truth. Use its terminology (`intent_state` / `resolved_state`, `mat_slot`, canonical token) instead of inventing synonyms, and introduce a new term in the chapter it belongs to.
- Design principles are cited as `P1`–`P5`, from [Design Principles](https://cairn.kage1020.com/spec/principles/).
- Error messages say *what is wrong / what would be valid / how to fix it*. That shape is what makes the write-check-fix loop work, so it is worth the extra sentence.
- Keep examples concrete and minimal.

### No session-local references

Rust source, spec prose, examples, and docs must still make sense on their own years from now. Leave out issue and PR numbers, `M3-PR4`-style coordinates, bare milestone labels, and "a later PR will…". Replace the coordinate with the fact it stood in for:

> Before: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land later).`
>
> After: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

When the deferred thing lands, update the comment in the same PR.

Three surfaces are exempt, because milestone vocabulary is what they are *for*: [CHANGELOG.md](CHANGELOG.md) with its fragments in [changelog.d/](changelog.d/README.md), the [roadmap](https://cairn.kage1020.com/roadmap/), and the [compatibility](https://cairn.kage1020.com/spec/compatibility/) page with its milestone columns. `cargo xtask lint session-refs` checks every other tracked text file, except this guide, which quotes the coordinates it asks you to leave out; the pull request template, which asks for the issue a PR closes; the changelog script's tests; and lockfiles.

### Cite the spec by name, not by number

A section number is the same kind of coordinate. `§14.5` is true only while "Place-and-route" happens to be the fifth section of the fourteenth chapter. Insert a chapter or split a section, and every comment that named it quietly becomes wrong; the only way to find them is to read the whole source tree. Editing the spec should not cost that. Name the chapter file and, when you mean one section, its title:

> Before: ``/// Stable per `spec/lint.md` §11.3: errors are things the compiler refuses to guess at.``
>
> After: ``/// Stable per `spec/lint` "Error vs warning": errors are things the compiler refuses to guess at.``

Write the chapter as its file stem without the extension (`spec/redstone`, `spec/versioning-editions`) and copy the section title verbatim. Cite the chapter alone when you mean the whole chapter. Once a comment has given the full citation, later sentences in it refer back in prose ("that pipeline", "the phase order") rather than repeat it.

`cargo xtask lint spec-citations` checks the `.rs`, `.md` and `.crn` files in `crates/`, `examples/`, `website/rust/`, `xtask/`, `.github/`, `.claude/` and `CLAUDE.md` for three things:

- no `§` or "section 11.3" appears in any of them;
- every `spec/<chapter>` names a chapter that exists;
- every quoted title is something `cairn docs` prints for that chapter: a heading, a term a list item in the chapter defines in bold (`- **Term.**`), or the chapter's own title.

So a citation runs as written, `cairn docs spec/lint "Error vs warning"`. A title that more than one part of the chapter has fails the rule, since it would not say which part it means. Renumbering the spec touches no Rust at all. Retitling a section fails the rule at each citation, by file and line, and that is the point: a title change is a change of meaning, and the comment that leaned on it deserves a re-read.

The rest of the repository is on you. The rule skips the pages under `website/`, because that is where the numbers are defined. The guide pages beside the spec still link to sections by anchors that bake the number into the slug; untangling that belongs with a change to the headings themselves. `editors/`, this file, `CHANGELOG.md`, `changelog.d/`, and files of other kinds, such as the scripts in `.github/scripts/`, are simply outside the scan.

### Crate READMEs

Each crate's `README.md` is what crates.io shows, so it is the front page for anyone who has never seen the repository. When a stage lands, update that crate's README in the same PR. A stage has not shipped while its front page still says it is coming.

Four crate READMEs keep inventories that can be compared against the code, and `cargo xtask lint` checks every one of them, not review:

- `cairn-lang-core`'s module table against the crate's `pub mod` declarations;
- `cairn-lang-cli`'s subcommand table against what `cairn --help` lists;
- `cairn-lang-cli`'s `cairn synth` section against the stages `--stage` offers, in their order, and the stage where `--edition` becomes required;
- the `## Public API` tables in `cairn-lang-nbt` and `cairn-lang-formats` against what each crate root re-exports.

Add a `pub mod`, a subcommand, a stage, or a `pub use` without its row and the lint fails, naming what is missing. Every check runs in both directions, so a row for something that no longer ships fails too.

`## Public API` means the crate root's `pub use` re-exports, and nothing else. An item that is only `pub` inside a module, such as `registry::AliasCatalog` or anything under `registry::blocks`, is deliberately left off the table; describe it in prose if it needs describing. A row writes one full `module::item` path per code span, and may hold several spans when the items share a sentence. Adding a `## Public API` table to a crate that has none means adding that crate to `PUBLIC_API_CRATES` in `xtask/src/rules/readme.rs`, which the same rule will tell you.

Beyond those lists, no rule reads the prose. A row's description, a "Status" paragraph, a heading calling a table *planned*, a "Not yet here" list: each stays true only as long as someone remembers to update it. Two published crates went on describing an unimplemented skeleton long after the compiler worked. When you move something out of "not yet", move it out of the README too. Say what ships today, and keep what is still ahead in a section of its own, so a reader can tell the two apart without running the code.

## Branches and pull requests

`canary` is the trunk. `main` is the branch releases are published from, and nothing is published from anywhere else. Once a month, the release workflow opens a promote-to-main PR from `canary`. Merging it opens the release PR on `main`, and merging that publishes `main`. The workflow then opens a PR that merges `main` back into `canary`, so `canary` carries every release commit and hotfix before the next promote.

| Branch | Purpose |
|---|---|
| `canary` | All feature work, fixes, and docs land here. Protected. |
| `main` | Moved by the promote-to-main PR, the release PR, and hotfix PRs, each approved by a maintainer. No direct pushes. |
| `<type>/<short-kebab>` | One change, targeting `canary`. Deleted after merge. |
| `hotfix/<short-kebab>` | A fix that cannot wait for the next promote, such as a security fix. Branched from `main`, targeting `main`, and opened only when a maintainer asks for it. Deleted after merge. |
| `release-plz-*` | Opened by the release workflow against `main`, each time `main` gains a promote or a hotfix that makes a release due. |

Name the branch after the Conventional Commits type the work will land under: `feat/parser-lexer`, `fix/wall-corner-shape`, `docs/roadmap-2027`.

**The PR title must be a [Conventional Commits](https://www.conventionalcommits.org/) line.** Every PR is squash-merged, except the two the release workflow opens between `canary` and `main`, so the title becomes the commit on the branch the PR targets, and `release-plz` reads it to decide whether a patch release is due. Commits on your own branch are free-form. The scope names the crate or spec area (`feat(core)`, `fix(nbt)`, `docs(spec)`, `build(deps)`).

| Type | When | Makes a patch release due? |
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

A patch release that is due goes out when a maintainer runs the release workflow by hand with `step: promote`, or when a hotfix lands on `main`. Only the monthly promote runs on a schedule.

**A breaking change puts `!` before the colon**, after the type or, when one is written, after the scope: `feat(core)!: replace lexer`, `fix!: …`. The `!` and the changelog always go together. A PR that adds a `changelog.d/<slug>.breaking.md` fragment titles itself with `!`, and a PR titled with `!` adds that fragment.

The hand-written entry is what [compatibility "How a break is communicated"](https://cairn.kage1020.com/spec/compatibility/) requires. Once the release folds it in under `### Breaking changes`, it is what a reader of `CHANGELOG.md` sees. The `!` records the same fact in the commit that `release-plz` parses. The `Changelog` check reports a PR that has one without the other. Until that check is required, a reviewer who sees one without the other asks for the missing half before merging.

The `!` does not choose the version. Semver would read it as a major bump, but here the release workflow computes the next `YYYY.M.PATCH` from the date and the existing tags, and writes it into `Cargo.toml` before `release-plz` runs. `release-plz` leaves alone a version that already differs from the published one. Nor does the `!` decide whether a release is due; the type does, per the table above. What it changes is the generated release notes. `release-plz` prefixes the commit's line with `[**breaking**]`, and `protect_breaking_commits` keeps that line even for a type whose lines are otherwise dropped (`docs!:`, `ci!:`).

Every PR you open targets `canary`, unless a maintainer asks for a hotfix. Merging needs one maintainer approval, and green CI is expected, although the branch rulesets do not require any check yet. The release PR follows the same rules.

A hotfix branches from `main` as `hotfix/<short-kebab>` and is titled and given a fragment like any other PR. Its merge into `main` opens the release PR for the next patch, which holds only what `main` has, and merging that publishes the fix. The PR back into `canary` then brings the fix to `canary`.

A maintainer merges the promote-to-main PR and the PR back into `canary` by hand, with a merge commit. A squash would leave `main`'s commits out of `canary`'s history, and the next promote refuses while `main` has a commit `canary` lacks. Neither PR turns on auto-merge, because a merge GitHub performs for the workflow's token starts no workflow, and the promote's merge has to start the release PR.

## Changelog entries

A PR does not edit [CHANGELOG.md](CHANGELOG.md). Instead it adds a fragment under [changelog.d/](changelog.d/README.md), one file per entry, named `<slug>.<kind>.md`, where the kind picks the heading (`breaking`, `deprecations`, `added`, `changed`, `fixed`, `performance`, `build`).

Fragments avoid conflicts. With twenty PRs open at once, every one that edits the same `[Unreleased]` heading conflicts with the next merge; separate files never do. The release PR, monthly or patch, folds the fragments into the section `release-plz` writes for the new version, then deletes them.

The `Changelog` workflow checks the name and shape of each fragment a PR adds or edits. It also reports issue or PR numbers, entries scoped to `ci` alone, and a line `CHANGELOG.md` gains or loses outside the release PR. [changelog.d/README.md](changelog.d/README.md) has the details and the commands to run the check locally. All of this applies to the root `CHANGELOG.md` only: `editors/vscode/CHANGELOG.md` belongs to the extension and is still edited by hand.

## Revisiting a settled decision

Some decisions are deliberately closed: `key=value` over positional arguments, phase-ordered evaluation, recompile-don't-transcode, fail-loud over silent substitution. To reopen one, file an issue that covers:

- the decision, and where it lives in the spec;
- a concrete case it fails;
- an alternative, with syntax, IR, and message examples;
- the effect on the [evaluation metrics](https://cairn.kage1020.com/spec/evaluation/).

## Bumping the toolchain

The toolchain is deliberately pinned to a version, not a channel. On `stable`, a Rust release turns every open branch red at once, with findings in files the branch never touched. Pinning does not avoid new lints; it makes them arrive in a pull request of their own.

Dependabot opens that pull request when a new Rust is released. It changes `channel` and is typed `ci`. Run the CI commands above on its branch and push the fixes for what they find onto the same branch. A new compiler can raise a *rustc* or *rustdoc* warning, not just a clippy lint, so a pin bump can turn `Docs` red too. A diff that is only "new compiler, plus the fixes it asked for" is one a reviewer can actually read.

The pin is not the MSRV. `rust-version` in the workspace manifest is the floor a consumer needs to build Cairn, and the pin is always newer. So a change that reaches for a recently stabilised API is green at the pin and broken at the floor.

Clippy catches part of this. `clippy::incompatible_msrv` reads `rust-version` and refuses a *standard library* item stabilised above it, and `-D warnings` makes that fatal. But it is a lint, so one `#[allow]` silences it. It also says nothing about a **dependency** whose own `rust-version` is above ours; that is a hard cargo error, and nothing built at the pin ever sees it.

CI's `MSRV` job compiles at the floor and so catches both. It reads `rust-version` back out of the manifest with `cargo metadata`, so there is no second copy to go stale, installs that compiler, and runs `cargo check --workspace --locked --all-features` with it:

- `check` rather than `test`, because the floor is about compiling the crates a consumer depends on. Dev-dependencies are free to want a newer compiler than the library does.
- `--all-features`, because `rust-version` is one declaration per package, and cargo has no way to say "this floor, unless you enable that feature".
- `--locked`, because the committed lockfile is what a consumer cloning the repo resolves to. When the failure is a dependency's own floor rather than Cairn's code, cargo names the package.

The workspace sets `resolver = "3"`, Cargo's MSRV-aware resolver. `cargo update` and a new dependency therefore pick the newest version whose own `rust-version` is at or below ours, and a dependency past the floor never reaches `Cargo.lock`. The key is written out because a virtual manifest does not take it from its members' edition and would fall back to resolver 1. The resolver only filters candidates that declare a `rust-version`. A dependency that declares none, or a version forced with `cargo update --precise`, still lands in the lockfile, and the `MSRV` job is what catches those.

Raising `rust-version` changes who can build the crates, so it is never a quiet manifest edit. Type the commit `build`, which makes a patch release due so the new floor reaches crates.io, and add a `changelog.d/<slug>.build.md` entry saying which compiler is now required and what needed it. A consumer pinned below the new floor learns about it from cargo either way; the entry tells them why.

## Changing the release profile

`[profile.release]` in the workspace manifest is tuned for the size of the release archives, and its comments say why. Most of its settings cost only build time. `opt-level` is the exception: a size-oriented level would shrink the archives but slow block-array lowering and place-and-route, which is where a build spends its time.

Two benches measure that throughput: `lowering` in `cairn-lang-core` and `place_and_route` in `cairn-lang-redstone`. They call the passes in-process, since timing the CLI over an example would mostly time process startup. They also generate their sources instead of using an example, since a source that small goes through a pass faster than the timer can tell a change apart.

Benches take their settings from the release profile, so `opt-level`, `lto` and `codegen-units` are timed as they ship. `panic` is the exception: Cargo builds bench targets and their dependencies with unwinding, as it does tests.

Save a baseline, change the profile (or override one setting from the environment without editing the file), and compare:

```sh
cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --save-baseline before
CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo bench -p cairn-lang-core -p cairn-lang-redstone --bench lowering --bench place_and_route -- --baseline before
```

Name the two benches: without `--bench`, `cargo bench` also runs each library's unit-test harness, which refuses criterion's `--save-baseline`. Run a baseline against itself once before trusting a difference. On a shared machine the smaller benches move by several percent between identical builds.

Weigh a size change on the gzipped binaries, since the release archives are `.tar.gz` and `.zip`. Put the numbers in the commit message, not the profile comment, where they would go stale on the next dependency bump.

CI times neither bench. A timing gate on shared runners would be noise, and a profile change is a decision made once, not a surface to watch for regressions. What CI does run is each bench once as an untimed test: `cargo test -p cairn-lang-core --bench lowering` and `cargo test -p cairn-lang-redstone --bench place_and_route`.

Before it times anything, each bench checks that its generated source does the work it was built for. `lowering` checks that every placement and walkway comes out of block-array lowering; `place_and_route` checks that every scope comes through every pass with one placed cell per gate. A generator that stopped reaching the passes therefore fails in CI instead of quietly timing less work.

## Versioning

Date-based, `YYYY.M[.PATCH]`. Notable changes go in [CHANGELOG.md](CHANGELOG.md). [Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/), not the number itself, set what a version bump may break.

## Code of Conduct

This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you agree to uphold it.
