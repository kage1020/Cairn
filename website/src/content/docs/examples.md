---
title: "Examples"
---

Worked Cairn (`.crn`) examples. Each one is referenced by [`docs/tutorial.md`](../docs/tutorial)
and is intentionally minimal so the language surface is the only thing on screen.

> The reference compiler is not implemented yet, so these examples are currently *normative
> illustrations*, not files you can build. They are kept under `cargo check` indirectly by being
> referenced from the spec and tutorial.

| File | Demonstrates |
|---|---|
| [`cottage.crn`](cottage.crn) | The minimum useful build: `struct` + `theme` + slots + selectors. |
| [`themed-tower.crn`](themed-tower.crn) | Abstract material tokens, per-floor levels, override-promotion. |
| [`redstone-door.crn`](redstone-door.crn) | Logical redstone: signal binding, `circuit` region, assertions. |
| [`village.crn`](village.crn) | Multi-building via `site` and topological `connect`. |

To follow along, read the tutorial first; it walks each file from top to bottom and references the
spec chapter behind every line.
