---
title: Roadmap
description: Date-driven, small-and-frequent releases. Monthly minor with patches as needed, and six named milestones from "source parses" to "redstone simulates", four of them reached.
---

Cairn ships **monthly minor releases** under
[date-based versioning](/spec/versioning-editions/#101-the-target-is-a-compile-time-parameter)
`YYYY.M[.PATCH]`. Each month delivers what is ready; nothing is held back for an arbitrary "1.0".
Six named milestones sit across the monthly schedule, so contributors and consumers can plan
against something more durable than a calendar date.

> The schedule is a plan, not a promise. The compiler is implemented in the open, and slips happen.
> The [CHANGELOG](https://github.com/kage1020/Cairn/blob/main/CHANGELOG.md) is the source of truth
> for what shipped.

## Where things stand

The compiler ran well ahead of the first plan. What M1, M2, M4 and M5 ask the compiler to do had
all landed by 2026.8.0, three of them months before their dates, and the releases since have
hardened it against the spec. M2 also makes the lockfile format Stable, and the lockfile kept
changing after M2's build landed: 2026.9.0 added `lock_schema_version` and reframed the registry
pack hash, and 2026.10.0 canonicalised the palette, which moved every `resolved_ir_hash`. So M2
counts as reached with 2026.11.0, the release its Stable contract starts from. Two milestones
remain.

- **M3** is where the language leaves draft. Every example already compiles clean for Java and
  builds for Bedrock. What M3 still needs is listed under [Leaving draft](#leaving-draft).
- **M6** is where redstone reaches the structure file. Synthesis and place-and-route run behind
  `cairn synth --experimental-logic-synth`, but no routed circuit is written into an artifact, and
  nothing simulates one.

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

Each milestone is a "the project can credibly do X" gate, hit when the release that does X ships.
A milestone that turns a surface Stable is reached when that surface's Stable contract starts.
Naming the gates separately from version numbers gives the roadmap a stable vocabulary even when
the monthly schedule shifts.

| Milestone | Status | The thing it earns |
|---|---|---|
| **M1, source parses** | Reached in 2026.7.1 | `cairn parse` produces an AST for every example in `examples/` |
| **M2, minimal build** | Reached in 2026.11.0; the build landed in 2026.7.1 | `cairn compile` writes a Java `.nbt` for a single-room structure with floor and walls, plus a lockfile, and the lockfile format turns Stable |
| **M3, examples work** | Planned for 2027.1.0 | Every example in `examples/` compiles clean with `--edition java` and loads in Minecraft; the specification leaves draft, and the surfaces the [compatibility tiers](/spec/compatibility/#c2-tier-of-each-surface-by-milestone) list for M3 turn Stable |
| **M4, Java/Bedrock parity** | Reached in 2026.8.0 | Same DSL source emits valid output for both editions; parity table populated; per-edition theme fallbacks work |
| **M5, developer experience** | Reached in 2026.8.0 | `cairn-lsp` provides diagnostics and completion in at least one editor (VS Code) |
| **M6, redstone simulates** | Planned for 2027.4.0 | `cairn compile` lays a `logic` graph into the structure file, and a tick simulator verifies `redstone-door`'s assertions against the circuit it built |

## Shipped so far

Each minor release by its headline. The patches and the full lists are in the CHANGELOG, which
files the first release under `2026.7.0`; its tag and GitHub release are `v2026.7.1`.

| Release | What it brought |
|---|---|
| **2026.7.1** | The first release. `cairn parse`, `check`, `info`, `lower` and `compile`; the Intent IR, the semantic layer and the block-array pivot; Java `.nbt` output with a lockfile; registry pack ingest and nearest-valid suggestions; `gable`, `shed`, `hip` and `flat` roofs; `site` placements joined by walkways. **M1**. |
| **2026.8.0** | Bedrock `.mcstructure` output, with the parity table and per-edition theme fallbacks. `themed-tower`, `village` and `redstone-door` compile clean, through `level` grouping and eave stairs, walkways routed around structures, and pressure plates, circuit region markers and door actuator patches. `cairn-lsp` with diagnostics and completion, and a VS Code extension; the tree-sitter grammar, on npm as `tree-sitter-cairn`; redstone synthesis, placement, routing, delay insertion and crossing legalization behind `cairn synth --experimental-logic-synth`. **M4**, **M5**. |
| **2026.9.0** | Hardening. A source that does not parse gets a diagnostic like any other; the lockfile is read back, so a rebuild for another target says so; the floor `@requires` declares is enforced; nesting and extents are bounded, so hostile input is refused instead of exhausting the compiler. |
| **2026.10.0** | Version floors in full: `@cairn` is read, `@requires` floors order by DataVersion and may name an edition, a `def` or a `theme` declares its own floor, and `@intended_targets` is weighed against them. A renamed block id is answered from the pack's alias table, and an `assert truth` row may use `-` as a don't-care. |
| **2026.11.0** | `cairn docs` prints the documentation built into the binary, offline, and the site serves each page as Markdown with an `/llms.txt` index. `cairn check` with no target reports the lowering findings that need none, a truth table may be written one row per line, and an even-span hip roof's apex faces away from its ridge. A pressure plate its line cannot place is the error `E_UNPLACEABLE_PLATE`, and `redstone-door`'s plates sit where a player can step on them, a name Windows reserves is refused on every host (`E_INVALID_PLACE_ID`, `E_INVALID_STRUCT_NAME`), and `and` / `or` / `not` come from a built-in logic library, which changes the expression shape `cairn parse --format json` prints. The lockfile format turns Stable. **M2**. |

## Monthly scope

The table below is the working plan, month by month, for the releases still ahead. It is denser
than the milestone table because a monthly minor can ship without crossing a milestone gate.

| Release | Scope added |
|---|---|
| **2026.12.0** | What M3 waits on ([Leaving draft](#leaving-draft)): unimplemented syntax flagged, the coordinate system settled, the language-evolution policy closed, tier-1 tokens marked, and the compatibility chapter's rule for retiring a placeholder code. Each release attaches the VS Code extension as a `.vsix`, and the extension is on the Marketplace. |
| **2027.1.0** | Every example compiles clean on Java and loads in Minecraft. The specification leaves draft as `2027.1`, and `.crn` syntax, the `compile` / `check` / `info` interface, the diagnostic codes and the tier-1 vocabulary turn Stable. **M3.** |
| **2027.2.0** | Blockstate derived beyond stairs: a `door` places its door block, panes, fences and walls connect, chests pair, and `fill`, with the coordinate triples it takes (`from=1,1,1`), sets `waterlogged`. The built-in packs are extracted from each edition's `--reports` / registry dumps, for the targets they carry today and each edition's current releases, so `E_STATE_DOMAIN` checks a state literal against the target and an error names the version an id first appears in. |
| **2027.3.0** | Redstone reaches the structure file: edition legalization and lowering from the Placement IR to the block array, so `cairn compile` runs the `logic_*` phases. The rest of the combinational primitives (`xor`, `nand`, `nor`, `mux`), the remaining sensors and actuators (`lever`, `button`, `daylight`, `observer`, `lamp`, `piston`, `dispenser`), Tier 0 parts such as `repeater` placed by hand, the `raw` escape hatch, and `@edition` alternatives with `E_PARITY_UNSUPPORTED`. |
| **2027.4.0** | A tick simulator checks `assert truth` and `assert always` against the built circuit; `assert latency`; the sequential macros (`latch`, `pulse`, `delay`, `edge_rising`, `edge_falling`, `counter`); `E_NO_PORTABLE_IMPL` for logic that depends on update order, whose fix, Tier 0 under an `@edition` guard, exists by then. `redstone-door` verifies, and the redstone DSL turns Stable. **M6.** |
| **2027.5.0** | Entities and block entities (`spawn`, `painting`, `sign`, `bed`, …) with their anchors, and the decimal (`scale=1.5`) and `nbt={...}` values they take; theme selector bindings, specified and lowered; the first constraint catalog (attachment, support, fluids, entity AABB), so `cairn info` reports semantic-sensitive members and a lock records the catalog's hash. |
| **2027.6.0** | `edit` patches, and the stable addresses the compiler gives members written without an `id=`; `.litematic` and `.schem` writers; `cairn-lang-wasm` and the browser playground (live compile in the docs site). |

Beyond `2027.6.0` the schedule is not drawn yet. The spec describes more than this plan reaches:
the reverse direction (`cairn import`, `cairn diff-blocks`), the harness the
[evaluation framework](/spec/evaluation/) calls for, and the open questions that wait on its data,
columns and arches as primitives and interiors. The roadmap will be redrawn around those, and
around feedback from real use, as 2027.6.0 approaches.

## Leaving draft

The specification is titled `2026.06 (draft)`. The label comes off at M3, when the surfaces the
[compatibility tiers](/spec/compatibility/#c2-tier-of-each-surface-by-milestone) list for M3 turn
Stable: `.crn` syntax (chapters 5 to 9), the flags, JSON schemas and exit codes of
`cairn compile`, `check` and `info`, the diagnostic codes, and the tier-1 material vocabulary. From
then on the specification carries the version of the release that ships it, starting with
`2027.1`, and a break to any of those surfaces waits out a `W_DEPRECATED` window.

What stands between today and that point:

- **Stable covers syntax the compiler builds.** Chapters 5 to 9 still describe syntax the compiler
  does not build yet: coordinate triples, decimals and `nbt={...}` values, blockstate overrides
  beyond a stair's, `fill`, entities and block entities, `edit`, and theme selector bindings, which
  parse and paint nothing. So do `@edition` guards, which chapter 10 specifies and
  [compatibility "Tier definitions"](/spec/compatibility/#c1-tier-definitions) counts among the
  Stable syntax. Each such section is flagged subject to change, which keeps it Evolving until it
  lands.
- **The coordinate system is settled.** Whether the corner origin and `front = +z` stay is still an
  [open issue](/spec/open-issues/#152-untouched-topics). Once syntax is Stable, moving the origin
  is a break, so it is decided first. Primitive promotion and interiors would only add vocabulary,
  so they can stay open past M3.
- **The language-evolution policy is closed.**
  [Open issues "Language-evolution policy"](/spec/open-issues/#153-language-evolution-policy) still
  weighs an opt-in, year-based mechanism against announcing changes per release. The label above
  takes the second answer, CalVer with the CHANGELOG and the compatibility tiers, so M3 records it
  as decided.
- **The tier-1 tokens are marked.** The
  [canonical vocabulary](/spec/materials-themes/#72-canonical-vocabulary) does not say yet which of
  its tokens are tier-1, and the tier-1 row is one of those turning Stable.
- **The diagnostic codes settle.** After M3, adding a code breaks nothing and changing what an
  existing code means does, so changes of the second kind land before 2027.1.0. A placeholder code
  that stops firing because the construct it stands in for lands, as `W_STATE_LITERAL_UNCHECKED`
  gives way to `E_STATE_DOMAIN` or `E_UNKNOWN_KEYWORD` stops covering `spawn`, counts as adding a
  code: what the placeholder means is unchanged, and what replaces it is the spec's own behaviour,
  which [compatibility "What is not covered by tier"](/spec/compatibility/#c4-what-is-not-covered-by-tier)
  never counts as a break. The compatibility chapter says so before M3.

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
   beside a sigstore bundle for each archive except the Windows `aarch64` one, and a signed
   CycloneDX SBOM; publish the tree-sitter grammar to npm; and open a PR that promotes `canary` to
   `main`.

Compatibility expectations across this schedule are defined separately by
[compatibility tier](/spec/compatibility/): the lockfile format is **Stable** from 2026.11.0, where
M2 is reached, and the LSP wire protocol from M5; `.crn` syntax follows at M3 and the redstone DSL
at M6; the Rust API stays **Internal** throughout, and most other surfaces sit at **Evolving** for
now.
