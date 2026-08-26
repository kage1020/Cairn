---
title: "Specification — 2026.06 (draft)"
description: The normative specification of Cairn, an intermediate language for reading and writing Minecraft builds.
---

This is the normative specification of **Cairn**, an intermediate language for AI to read and write
Minecraft builds. It avoids the inefficiency of NBT/SNBT — binary, one record per block — and aligns
architectural knowledge (walls, roofs, symmetry) with the voxel world. The approach is
**generation-first**, and therefore lossy on purpose.

## Reading order

| # | Chapter | Contents |
|---|---|---|
| 1 | [Purpose and Scope](overview) | Purpose, scope, non-goals |
| 2 | [Design Principles](principles) | P1–P5 |
| 3 | [Architecture](architecture) | Three-layer IR, block-array pivot |
| 4 | [Compilation Model](compilation) | Phase evaluation, target axes, roof lowering |
| 5 | [Syntax](syntax) | Lexical rules, `key=value`, selectors, headers |
| 6 | [Blockstate Model](blockstate) | Derivation and override, intent vs resolved, waterlogged |
| 7 | [Materials and Themes](materials-themes) | Slots, canonical vocabulary, themes |
| 8 | [Entities](entities) | Two-tier entity model, anchor conventions |
| 9 | [Components, Editing, and Multi-building](components-editing-sites) | `def`, editing, `site` |
| 10 | [Versioning and Editions](versioning-editions) | Target strategy, lock, portability |
| 11 | [Lint](lint) | Diagnostic codes and constraint validation |
| 12 | [Ecosystem Interop](ecosystem-interop) | Import, reverse conversion |
| 13 | [Evaluation Framework](evaluation) | How the spec is iterated |
| 14 | [Redstone](redstone) | Logic circuits |
| 15 | [Open Issues](open-issues) | What is still undecided |
| — | [Compatibility Tiers](compatibility) | Stable / Evolving / Internal, per public surface |
| — | [Glossary](glossary) | Defined terms, cross-chapter |

## Conventions

- Requirement words **MUST / SHOULD / MUST NOT / OPTIONAL** are used in the RFC 2119 sense.
- The language is **Cairn**, the CLI is `cairn`, and source files use `.crn`.
- Design principles are referenced as `P1`–`P5` (see [Design Principles](principles)).

## Two version axes

Cairn's own releases use date-based versioning (CalVer) `YYYY.M[.PATCH]` — `2026.7` for a monthly
release, `2026.7.1` for an in-month patch. They sort chronologically as strings. A release bundles
the language spec, the reference compiler, the standard library, and the `(edition, version)`
registry and constraint catalogs.

**Minecraft also moved to date-based versions**, so the two cannot be told apart by format. They are
distinguished by field, flag, or keyword — never by shape:

| | Cairn's own version | The Minecraft target |
|---|---|---|
| Lock | `cairn_version` | `mc_version` |
| Headers | `@cairn` | `@requires`, `@intended_targets` |
| CLI | `cairn --version` | `--target` |

In prose, disambiguate with a prefix: `cairn:2026.06` and `mc:1.21.4`.

This document is **2026.6 (draft)**, superseding the former `v0.2` label. A `.crn` file MAY declare
`@cairn 2026.06` — the language version it was written against. It is provenance only, so a future
compiler can parse and warn correctly. See [Syntax §5.3](syntax#53-headers).
