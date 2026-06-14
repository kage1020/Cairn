---
title: Roadmap
description: Date-driven, small-and-frequent releases. Monthly minor with patches as needed, six named milestones from "source parses" to "redstone simulates".
---

Cairn ships **monthly minor releases** under [date-based versioning](spec/versioning-editions#101-the-target-is-a-compile-time-parameter)
`YYYY.0M[.PATCH]`. Each month delivers what is ready; nothing is held back for an arbitrary "1.0".
Six named milestones cross-cut the monthly schedule so contributors and consumers can plan against
something more durable than a calendar tick.

> Schedule is a plan, not a promise. The compiler is implemented in the open and slips happen.
> The [CHANGELOG](https://github.com/kage1020/Cairn/blob/main/CHANGELOG.md) is the source of truth
> for what actually shipped.

## Release cadence

- **Monthly minor** (`2026.MM.0`) on the first of the month. The release PR is opened by a cron
  job and merged after human review.
- **Patches** (`2026.MM.N`, `N ≥ 1`) are cut on demand from `main` whenever a relevant commit
  lands. Typical triggers: registry/constraint pack updates, regressions, and security fixes.
  No upper bound on patches per month.
- **Channel:** there is only one — `stable`. Cairn does not run a separate nightly or beta train.
  Behaviour that is not yet stable is gated by [compatibility tier](spec/compatibility), not by
  release channel.
- **Backports:** none. The latest release is the supported release. Older `2026.MM.*` lines do
  not receive further patches.

## Milestones

Each milestone is a "the project can credibly do X" gate, hit when the corresponding month ships.
Naming the gates separately from version numbers gives the roadmap a stable vocabulary even when
the monthly schedule shifts.

| Milestone | Lands by | The thing it earns |
|---|---|---|
| **M1 — source parses** | 2026.07.0 | `cairn parse` produces an AST for every example in `examples/` |
| **M2 — minimal build** | 2026.10.0 | `cairn compile` writes a Java `.nbt` for a single-room structure with floor and walls, plus a lockfile |
| **M3 — examples work** | 2027.01.0 | `cottage`, `themed-tower`, `village` all round-trip through `cairn compile --edition java` and load in Minecraft |
| **M4 — Java/Bedrock parity** | 2027.02.0 | Same DSL source emits valid output for both editions; parity table populated; per-edition theme fallbacks work |
| **M5 — developer experience** | 2027.03.0 | `cairn-lsp` provides diagnostics and completion in at least one editor (VS Code) |
| **M6 — redstone simulates** | 2027.05.0 | Logical redstone synthesis, place-and-route, and tick simulator land together; `redstone-door` example verifies |

## Monthly scope

The month → release contents table below is the working plan. It is denser than the milestone
table because monthly minors can ship without crossing a milestone gate.

| Release | Scope added |
|---|---|
| **2026.07.0** | `cairn-core` lexer/parser, `cairn parse` subcommand (AST display only). Release automation goes live. |
| **2026.08.0** | Intent IR; syntactic validation; `cairn check`. |
| **2026.09.0** | Semantic layer; materials and themes basics; `cairn info` reports the three version axes. |
| **2026.10.0** | Block-array pivot; Java backend (walls and floors only); lockfile (`build.cairn.lock`). |
| **2026.11.0** | `cottage.crn` compiles end-to-end for `--edition java`. |
| **2026.12.0** | Registry pack ingest; fail-loud errors with nearest-valid suggestions. |
| **2027.01.0** | All `examples/` work on Java. **M3.** |
| **2027.02.0** | Bedrock backend with parity table and per-edition theme fallbacks. **M4.** |
| **2027.03.0** | `cairn-lsp` minimal (diagnostics + completion); VS Code extension. **M5.** |
| **2027.04.0** | Redstone logical layer; combinational synthesis and place-and-route. |
| **2027.05.0** | Redstone tick simulator; sequential macros; `redstone-door` verifies. **M6.** |
| **2027.06.0** | `cairn-wasm` + browser playground (live compile in the docs site). |

Beyond `2027.06.0` the schedule is intentionally not drawn. Once M6 lands the spec-driven part of
the project is largely complete and the roadmap will be redrawn around real usage feedback.

## How the schedule is enforced

The release strategy itself is automated:

1. **Monthly minor PR** is opened by a GitHub Actions cron at `17:04 UTC` on the first of each
   month (deliberately offset from the hour boundary, where GHA crons are most likely to be
   delayed or skipped).
2. **Version is computed** from existing tags: no `v2026.MM.*` tag yet → next is `2026.MM.0`,
   otherwise the next in-month patch.
3. **release-plz** generates the version bump and changelog, applies the computed version via
   `--version-overrides`, and opens a PR.
4. **Human review** is mandatory for the monthly minor. Patches from `main` push events follow
   the same PR flow; merging triggers the publish pipeline.
5. **Publish** runs on the resulting `v*` tag: cross-compile binaries for Linux, macOS, and
   Windows on `x86_64` and `aarch64`; sign with sigstore; attach to a GitHub Release; and
   `cargo publish` every workspace crate to crates.io.

Compatibility expectations across this schedule are defined separately by
[compatibility tier](spec/compatibility): `.crn` syntax and the lockfile evolve under **Stable**
rules, the Rust API stays **Internal** (`#[doc(hidden)]`) until M3, and most other surfaces sit at
**Evolving** for now.
