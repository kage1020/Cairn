---
title: Introduction
description: What Cairn is, what it is not, and where to read next.
---

Cairn is a **description language for Minecraft builds**. You declare *intent* — walls, roofs,
windows, symmetry, themes, redstone logic — and the compiler resolves the voxels: blockstates,
orientations, coordinate math, signal routing, and per-edition/per-version block IDs.

A *cairn* is a deliberately stacked pile of stones that marks a place. A Minecraft build is
exactly that — intentionally placed blocks. The name is the thesis.

## What it solves

Minecraft's NBT/SNBT is **inefficient for AI to read and write** (binary, one-record-per-block)
and **misaligned with how humans and AI reason about architecture** (walls, roofs, symmetry).
Cairn sits between an AI's general architectural knowledge and Minecraft's voxel world.

The portable artifact is always the Cairn source; emitted NBT and schematics are per-target
build outputs, the equivalent of a compiled binary. Targeting a new Minecraft version means
**recompiling the source**, not transcoding the NBT.

## What it deliberately does not do

- **Full round-trip fidelity with NBT.** Generation-first is lossy by design. Imported
  schematics are kept as a faithful low-level transliteration that an LLM can lift into
  idiomatic Cairn, with a voxel-diff loop driving convergence.
- **General sequential synthesis of arbitrary state machines or CPUs in redstone.** v1 ships
  combinational gates plus a curated macro library; everything else drops to physical
  placement or a `raw` escape hatch.
- **Pre-1.13 numeric-ID `.schematic` import.** v1 does not support flattening.

See [Purpose and Scope](/spec/overview/) for the normative wording.

## Where to read next

- **Tutorial** ([English](/tutorial/), [日本語](/ja/tutorial/)) — a walk through the worked
  [Examples](/examples/) ([source on GitHub](https://github.com/kage1020/Cairn/tree/main/examples)).
- **Specification** ([English](/spec/), [日本語](/ja/spec/)) — fifteen chapters plus a
  cross-cutting glossary.
- **Developer Guide** ([English](/development/)) — the Rust workspace, how the crates are split,
  and how to land changes.
- **Playground** ([English](/playground/), [日本語](/ja/playground/)) — a placeholder for the
  browser-hosted compiler. Tracks `cairn-lang-wasm`.

## Status

Cairn is at the **design stage**, draft `2026.06`. The language is being designed in the open
and the reference compiler is a skeleton — the most useful contributions right now are design
critique, concrete proposals, and worked examples. See
[`CONTRIBUTING.md`](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md).
