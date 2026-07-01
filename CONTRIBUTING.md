# Contributing to Cairn

> Language: **English** ([日本語](CONTRIBUTING.ja.md))

Thanks for your interest in Cairn! The project is at the **design stage**: there is a normative
[specification](https://cairn.kage1020.com/spec/) (source under
[`website/src/content/docs/spec/`](website/src/content/docs/spec/)) and not yet a reference
implementation. The most valuable contributions right now are design critique, concrete
proposals, and worked examples.

## Ways to contribute

- **Design discussion.** Challenge a decision in the spec, surface a missing case, or propose an
  alternative. Open an issue that points at the specific chapter/section.
- **Worked examples.** Write `.crn` snippets for real builds and note where the language is awkward,
  ambiguous, or insufficient. These drive the vocabulary.
- **Spec edits.** Fix errors, clarify wording, improve examples. Keep each chapter self-contained and
  cross-link with relative links.
- **Prior art.** Pointers to redstone compilers, schematic formats, voxel/CAD place-and-route, and
  HDL synthesis are welcome in design discussions.

## Working language

The canonical language of the specification and project documentation is **English**. Translations
are welcome as clearly-labeled secondary copies, but English is the source of truth.

## Conventions

- The spec is the source of truth. Do not introduce session-specific identifiers, issue/PR numbers, or
  references that require external context to understand a passage later.
- Use the defined terminology (`intent_state` / `resolved_state`, `mat_slot`, canonical token, etc.)
  consistently. Introduce new terms in the relevant chapter, not ad hoc.
- Reference design principles as `P1`–`P5` (see
  [Design Principles](https://cairn.kage1020.com/spec/principles/)).
- Keep examples concrete and minimal. Error messages should be in the "what is wrong / valid
  alternatives / suggested fix" shape so they can feed the self-correction loop.

### Milestone and PR tag scope

The session-specific identifier ban above has a small set of deliberate exceptions, listed here so
the call does not get re-invented per review. The split follows the role each surface plays:
[CHANGELOG.md](CHANGELOG.md) and the
[Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/) §C.2 table hold the project's
historical and roadmap-aligned vocabulary, while Rust source and spec prose describe the
*implemented* behaviour and so must read independently of any one PR.

**Allowed to carry `MN-PRk`, `pre-MN`, `later PR`, or specific `YYYY.MM.0` references:**

- [CHANGELOG.md](CHANGELOG.md) / [CHANGELOG.ja.md](CHANGELOG.ja.md), including the `[Unreleased]`
  section. `release-plz` only appends to these files; existing tags stay untouched.
- The [Roadmap](https://cairn.kage1020.com/roadmap/) (`website/src/content/docs/roadmap.md` and
  the `ja/` mirror) — the roadmap is the milestone vocabulary.
- The C.2 milestone columns of
  [`spec/compatibility.md`](website/src/content/docs/spec/compatibility.md)
  (`Today (pre-M1) | At M2 (minimal build) | At M3 (examples work) | At M5 (DX) | At M6 (redstone)`)
  and the matching `ja/spec/compatibility.md` header. These are the table's axis labels.
- Git release tags (`v2026.MM.0`) and `release-plz.toml`.

**Must not carry these tags:**

- Rust source under `crates/**/*.rs` (comments and docstrings).
- Spec body under `website/src/content/docs/spec/**/*.md` and the `ja/` mirror, with the single
  C.2 header exception above.
- [`examples/`](examples/) `.crn` files.
- README, this file, and any other docs body.

**Rewriting guide.** Replace the PR coordinate with the implementation fact it stood in for:

- Before: `// M3-PR4 only exposes ports on door members (window / stair / roof ports land in a later PR).`
- After: `// Ports are currently exposed only on door members. Window / stair / roof ports are reserved for a future extension.`

The rule of thumb is "describe what the code does now, plus what is intentionally not yet covered,"
not "name the PR that delivered it." When a comment turns stale (the deferred feature has since
landed), update the comment in the same PR that lands the feature.

**Enforcement.** Reviewers (human or otherwise) should run

```sh
rg '\bM[1-6]\b|M[0-9]-PR[0-9]+|pre-M[0-9]|\blater PR\b|\bfuture PR\b' \
  --glob '!CHANGELOG*' \
  --glob '!CONTRIBUTING*' \
  --glob '!**/compatibility.md' \
  --glob '!**/roadmap.md' \
  --glob '!target/**'
```

before approving. An empty result is the contract. The `\bM[1-6]\b` arm catches bare milestone
labels (`M2`, `M3`, ...) — these belong to the roadmap and compatibility table only; in Rust source
and spec body, replace them with the implementation fact they stood in for ("the keyword table",
"the lowering pass", "reserved for a future extension"). This is not wired into CI yet; the
repository is small enough that human review is sufficient.

## Proposing a change to a settled decision

Several decisions are deliberately settled (e.g. key=value over positional args, phase-ordered
evaluation, recompile-don't-transcode, fail-loud over silent substitution). To revisit one, open an
issue that:

1. States the decision and where it lives in the spec.
2. Gives the concrete case it fails to handle.
3. Proposes an alternative with syntax/IR/message examples.
4. Notes the impact on the evaluation metrics (see
   [Evaluation Framework](https://cairn.kage1020.com/spec/evaluation/)).

## Branching and pull requests

Cairn uses a **`canary` trunk + `main` release pointer** layout. All ongoing work lives on
`canary`; `main` is updated automatically only when a release is published, so its history is
exactly the list of public releases.

### Branches

| Branch | Purpose | Lifetime |
|---|---|---|
| `canary` | The trunk. All feature work, fixes, docs, and the `release-plz-*` rolling release PR land here. Protected. | Permanent |
| `main` | The released state. Fast-forwarded to `canary` automatically right after each successful release. Protected. No direct pushes, no PRs. | Permanent |
| `<type>/<short-kebab>` | Working branch for a single change. Targets `canary`. | Until PR merge, then deleted |
| `release-plz-*` | Opened automatically by `release-plz` against `canary`, for monthly minors and patches. | Until PR merge |

Use the same `<type>` as the Conventional Commits type the work will land under
(`feat/parser-lexer`, `fix/wall-corner-shape`, `docs/roadmap-2027`, `refactor/ir-pivot`).

### Pull requests

- **All PRs target `canary`.** PRs against `main` are not accepted; `main` is updated only by
  the release pipeline.
- **PR title MUST be a [Conventional Commits](https://www.conventionalcommits.org/) line.**
  Examples: `feat(core): add lexer`, `fix(formats): correct big-endian NBT length`,
  `docs(spec): clarify §6.3`, `feat(redstone)!: rewrite tick simulator`. Individual commits on a
  feature branch are free-form.
- **Squash merge is the only allowed merge mode.** The PR title becomes the commit message on
  `canary`, which `release-plz` parses (`release_commits` regex in `release-plz.toml`) to decide
  whether a patch release is needed.
- Use the `!` suffix (e.g. `feat(core)!: replace lexer`) for breaking changes; this routes them
  to the `Breaking changes` section per
  [Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/) §C.3.
- One approval from a maintainer is required before merging. CI (fmt + clippy + test on Linux,
  macOS, Windows) must be green.
- The release PR (`release-plz-*` → `canary`) follows the same review rules. The monthly minor
  PR is opened by cron on the 1st of each month and merged after human review. Merging it
  triggers publish *and* fast-forwards `main`.

Recognized Conventional Commits types in Cairn:

| Type | When | Triggers a patch release? |
|---|---|---|
| `feat` | New feature, new public API, new subcommand | Yes |
| `fix` | Bug fix aligning behaviour with the spec | Yes |
| `perf` | Performance improvement | Yes |
| `refactor` | Internal restructuring without behaviour change | Yes |
| `build` | Build system, packaging, Cargo dependencies | Yes |
| `docs` | Documentation, spec prose, README, examples | No |
| `test` | Test code only | No |
| `ci` | GitHub Actions, release-plz, workflow config | No |
| `chore` | Anything else that doesn't ship to users | No |
| `style` | Formatting / lint-only changes | No |

A scope in parentheses identifies the affected crate or spec area: `feat(core)`, `fix(nbt)`,
`docs(spec)`, `build(deps)`.

## Versioning

Cairn uses date-based versioning (CalVer) `YYYY.M[.PATCH]`. Notable changes are recorded in
[CHANGELOG.md](CHANGELOG.md). The compatibility contract behind each surface is set by
[Compatibility Tiers](https://cairn.kage1020.com/spec/compatibility/), not by the version number.

## Code of Conduct

This project adheres to the [Contributor Covenant](CODE_OF_CONDUCT.md). By participating, you are
expected to uphold it.
