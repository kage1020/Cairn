---
title: "1. Purpose and Scope"
---

## 1.1 Purpose

Minecraft's NBT/SNBT is inefficient for an AI to handle directly — it is binary, and it is a flat
list of one record per block. It is also pitched at the wrong granularity: people and AI reason
about walls, roofs, and symmetry, not about individual voxels.

Cairn is an intermediate language that aligns an AI's general architectural knowledge with
Minecraft's voxel world — the eyes and hands an AI uses to see and build.

## 1.2 Approach: generation-first

Full round-trip fidelity with NBT is given up. The top priority is letting an AI generate and edit
builds accurately, which makes the language lossy by design.

The reverse direction — NBT or schematic back into Cairn — is best-effort
([Ecosystem Interop](ecosystem-interop)).

The portable artifact is always the **Cairn source**. Emitted NBT and schematics are per-target
build outputs, the equivalent of a compiled binary
([Versioning and Editions](versioning-editions)).

## 1.3 Scope and non-goals

Redstone can be described at the logic level: declare signals, gates, and connections, and the
compiler synthesizes, places, and routes them ([Redstone](redstone)). v1 covers combinational logic
plus a curated set of sequential macros. General sequential synthesis of arbitrary state machines or
CPUs is out of scope — those drop to Tier 0 physical placement or `raw`.

Also out of scope for v1:

- Full NBT recovery — chest contents, command blocks, and the like.
- Importing legacy numeric-ID `.schematic` files from before the 1.13 flattening.
