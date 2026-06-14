# Cairn — Minecraft Build DSL Specification 2026.06 (draft)

**Cairn** (a cairn is a deliberately stacked pile of stones that marks a place) is the normative
specification of an intermediate language for AI to read and write Minecraft builds. It avoids the
inefficiency of NBT/SNBT (binary, one-record-per-block) and aligns architectural knowledge (walls,
roofs, symmetry) with the voxel world. The approach is **generation-first (lossy)**.

## Reading order

| # | File | Contents |
|---|---|---|
| 1 | [overview.md](overview.md) | Purpose, scope, non-goals |
| 2 | [principles.md](principles.md) | Design principles P1–P5 |
| 3 | [architecture.md](architecture.md) | Three-layer IR + block-array universal pivot |
| 4 | [compilation.md](compilation.md) | Phase evaluation, target axes |
| 5 | [syntax.md](syntax.md) | Lexical, key=value, selectors, headers |
| 6 | [blockstate.md](blockstate.md) | Derivation + override, intent/resolved, waterlogged |
| 7 | [materials-themes.md](materials-themes.md) | Slots, canonical vocabulary, themes |
| 8 | [entities.md](entities.md) | Two-tier entity model, anchor conventions |
| 9 | [components-editing-sites.md](components-editing-sites.md) | def, editing, multi-building |
| 10 | [versioning-editions.md](versioning-editions.md) | Version/edition strategy, lock |
| 11 | [lint.md](lint.md) | Lint and constraint validation |
| 12 | [ecosystem-interop.md](ecosystem-interop.md) | Ecosystem interop, reverse conversion |
| 13 | [evaluation.md](evaluation.md) | Evaluation framework |
| 14 | [redstone.md](redstone.md) | Redstone (logic circuits) |
| 15 | [open-issues.md](open-issues.md) | Open issues |

## Terminology and conventions
- Requirement-level words: **MUST / SHOULD / MUST NOT / OPTIONAL** (RFC 2119 sense).
- The language name is **Cairn**, the CLI tool is `cairn`, and source files use the `.crn` extension.
- Design principles are referenced as `P1`–`P5` (see [principles.md](principles.md)).

## Versioning
Cairn's own releases use **date-based versioning (CalVer)** `YYYY.0M[.PATCH]`.
- Examples: `2026.06` (monthly release), `2026.06.1` (in-month patch). Sorts chronologically as a string.
- This document is **2026.06 (draft)**, superseding the former `v0.2` label.
- A release bundles "language spec + reference compiler + standard library + `(edition,version)`
  registry/constraint catalogs". It appears in `cairn --version` and the lock's `cairn_version`
  (see [versioning-editions.md](versioning-editions.md)).

**Separate axis from the Minecraft target version** (do not conflate):
- **Cairn version** `2026.06` — the release of the Cairn tool itself.
- **MC target** — the output Minecraft (`--edition java --target <version>`; see [versioning-editions.md](versioning-editions.md)).

**Minecraft itself moved to date-based versions from its latest release, so the two cannot be told
apart by format.** Versions are ALWAYS distinguished **by field/flag/keyword**:
- lock: `cairn_version` vs `mc_version`
- headers: `@cairn` vs `@requires` / `@intended_targets`
- CLI: `cairn --version` (Cairn itself) vs `--target` (MC)

When ambiguous in prose, use a prefix: `cairn:2026.06` / `mc:<version>`.

A `.crn` file MAY declare `@cairn 2026.06` (the Cairn language version it was written against). This is
a separate axis from the MC-version headers `@requires` / `@intended_targets`, and exists as
provenance so a future compiler can parse/warn correctly (see [syntax.md](syntax.md)).
