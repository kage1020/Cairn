---
title: Introduction
description: What Cairn is, what it deliberately is not, and where to read next.
---

Cairn is a description language for Minecraft builds. You declare intent — walls, roofs, windows,
symmetry, themes, redstone logic — and the compiler resolves the voxels: blockstates, orientations,
coordinate maths, signal routing, and the right block IDs for the edition and version you are
building for.

A *cairn* is a deliberately stacked pile of stones that marks a place. A Minecraft build is exactly
that. The name is the thesis.

## What it solves

Minecraft's NBT/SNBT is inefficient for an AI to read and write — it is binary, and it is one record
per block. It is also pitched at the wrong granularity: people and AI reason about walls, roofs, and
symmetry. Cairn sits between an AI's general architectural knowledge and Minecraft's voxel world.

The portable artifact is always the Cairn source. Emitted NBT and schematics are per-target build
outputs, the equivalent of a compiled binary — so targeting a new Minecraft version means
**recompiling the source**, not transcoding the NBT.

## What it deliberately does not do

**Full round-trip fidelity with NBT.** Generation-first is lossy by design. Imported schematics are
kept as a faithful low-level transliteration that an LLM can lift into idiomatic Cairn, with a
voxel-diff loop driving convergence.

**General sequential synthesis of arbitrary state machines or CPUs in redstone.** v1 ships
combinational gates plus a curated macro library; everything else drops to physical placement or a
`raw` escape hatch.

**Pre-1.13 numeric-ID `.schematic` import.** v1 does not support flattening.

See [Purpose and Scope](/spec/overview/) for the normative wording.

## Where to read next

| | |
|---|---|
| [Tutorial](/tutorial/) | A walk from one cottage to a village, through the worked [examples](/examples/). |
| [Specification](/spec/) | Fifteen chapters plus a cross-cutting glossary. |
| [Developer Guide](/development/) | The Rust workspace, how the crates split, and how to land changes. |
| [Playground](/playground/) | A placeholder for the browser-hosted compiler. Tracks `cairn-lang-wasm`. |

## Status

Cairn is at the design stage, draft `2026.06`. The language is being designed in the open and the
reference compiler is a skeleton, so the most useful contributions right now are design critique,
concrete proposals, and worked examples. See
[`CONTRIBUTING.md`](https://github.com/kage1020/Cairn/blob/main/CONTRIBUTING.md).
