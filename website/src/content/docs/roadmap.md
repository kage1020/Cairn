---
title: Roadmap
description: Date-driven, small-and-frequent releases. Monthly minor with patches as needed, six named milestones from "source parses" to "redstone simulates".
---

Cairn ships **monthly minor releases** under
[date-based versioning](/spec/versioning-editions/#101-the-target-is-a-compile-time-parameter)
`YYYY.M[.PATCH]`. Each month delivers what is ready; nothing is held back for an arbitrary "1.0".
Six named milestones sit across the monthly schedule, so contributors and consumers can plan
against something more durable than a calendar date.

> The schedule is a plan, not a promise. The compiler is implemented in the open, and slips happen.
> The [CHANGELOG](https://github.com/kage1020/Cairn/blob/main/CHANGELOG.md) is the source of truth
> for what shipped.

## Release cadence

- **Monthly minor** (`YYYY.M.0`) on the first of the month. The release PR is opened by a cron
  job and merged after human review.
- **Patches** (`YYYY.M.N`, `N ≥ 1`) are cut on demand: a maintainer starts the same release job
  by hand, and it opens a release PR from `canary`. Typical triggers: registry/constraint pack
  updates, regressions, and security fixes. No upper bound on patches per month.
- **Channel:** there is only one, `stable`. Cairn does not run a separate nightly or beta train.
  Behaviour that is not yet stable is gated by [compatibility tier](/spec/compatibility/), not by
  release channel.
- **Backports:** none. The latest release is the supported release. Older `YYYY.M.*` lines do
  not receive further patches.

## Milestones

Each milestone is a "the project can credibly do X" gate, hit when the corresponding month ships.
Naming the gates separately from version numbers gives the roadmap a stable vocabulary even when
the monthly schedule shifts.

| Milestone | Lands by | The thing it earns |
|---|---|---|
| **M1, source parses** | 2026.7.0 | `cairn parse` produces an AST for every example in `examples/` |
| **M2, minimal build** | 2026.10.0 | `cairn compile` writes a Java `.nbt` for a single-room structure with floor and walls, plus a lockfile |
| **M3, examples work** | 2027.1.0 | `cottage`, `themed-tower`, `village` all round-trip through `cairn compile --edition java` and load in Minecraft |
| **M4, Java/Bedrock parity** | 2027.2.0 | Same DSL source emits valid output for both editions; parity table populated; per-edition theme fallbacks work |
| **M5, developer experience** | 2027.3.0 | `cairn-lang-lsp` provides diagnostics and completion in at least one editor (VS Code) |
| **M6, redstone simulates** | 2027.5.0 | Logical redstone synthesis, place-and-route, and tick simulator land together; `redstone-door` example verifies |

## Monthly scope

The table below is the working plan, month by month. It is denser than the milestone table because
a monthly minor can ship without crossing a milestone gate.

| Release | Scope added |
|---|---|
| **2026.7.0** | `cairn-lang-core` lexer/parser, `cairn parse` subcommand (AST display only). Release automation goes live. |
| **2026.8.0** | Intent IR; syntactic validation; `cairn check`. |
| **2026.9.0** | Semantic layer; materials and themes basics; `cairn info` reports the three version axes. |
| **2026.10.0** | Block-array pivot; Java backend (walls and floors only); lockfile (`<source>.lock`). |
| **2026.11.0** | `cottage.crn` compiles end-to-end for `--edition java`. |
| **2026.12.0** | Registry pack ingest; fail-loud errors with nearest-valid suggestions. |
| **2027.1.0** | All `examples/` work on Java. **M3.** |
| **2027.2.0** | Bedrock backend with parity table and per-edition theme fallbacks. **M4.** |
| **2027.3.0** | `cairn-lang-lsp` minimal (diagnostics + completion); VS Code extension. **M5.** |
| **2027.4.0** | Redstone logical layer; combinational synthesis and place-and-route. |
| **2027.5.0** | Redstone tick simulator; sequential macros; `redstone-door` verifies. **M6.** |
| **2027.6.0** | `cairn-lang-wasm` + browser playground (live compile in the docs site). |

Beyond `2027.6.0` the schedule is intentionally not drawn. Once M6 lands, the spec-driven part of
the project is largely complete, and the roadmap will be redrawn around feedback from real use.

## How the schedule is enforced

The release strategy itself is automated:

1. **Monthly minor PR** is opened by a GitHub Actions cron at `04:17 UTC` on the first of each
   month. The time is deliberately off the hour, because GitHub Actions crons are most likely to be
   delayed or skipped on the hour. A patch runs the same job by hand, optionally with the version
   to use.
2. **Version is computed** from existing tags: no `vYYYY.M.*` tag yet → next is `YYYY.M.0`,
   otherwise the next in-month patch. The job writes it into the workspace `Cargo.toml`.
3. **release-plz** writes the changelog section from the commits and opens the release PR. The job
   then brings the tree-sitter and VS Code manifests to the same version, regenerates the parser,
   and folds the `changelog.d/` fragments into `CHANGELOG.md`.
4. **Human review** merges the release PR into `canary`, for a patch as for a minor.
5. **Publish** runs on that merge: `cargo publish` every crate except `xtask` to crates.io and tag
   `vYYYY.M.N`; build binaries for Linux, macOS, and Windows on `x86_64` and `aarch64`, each target
   on a runner of its own, and attach every archive with its SHA-256 checksum to a GitHub Release,
   beside a sigstore bundle for each archive except the Windows `aarch64` one; publish the
   tree-sitter grammar to npm; and open a PR that promotes `canary` to `main`.

Compatibility expectations across this schedule are defined separately by
[compatibility tier](/spec/compatibility/): the lockfile format turns **Stable** at M2 and `.crn`
syntax at M3, the Rust API stays **Internal** throughout, and most other surfaces sit at
**Evolving** for now.
