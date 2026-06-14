---
title: "1. Purpose and Scope"
---

## 1.1 Purpose
- Minecraft's NBT/SNBT is inefficient for AI to handle directly (binary; a flat list of
  one-record-per-block) and is misaligned with the granularity of architectural knowledge (walls,
  roofs, symmetry).
- Cairn is an **intermediate language that aligns an AI's general architectural knowledge with
  Minecraft's voxel world** — the "eyes and hands" an AI uses to see and build.

## 1.2 Approach: generation-first (lossy)
- Full round-trip fidelity with NBT is **given up**. The top priority is letting an AI generate and
  edit builds accurately.
- The reverse direction (NBT/schematic → Cairn) is best-effort ([ecosystem-interop.md](ecosystem-interop)).
- The portable artifact is always the **Cairn source**; emitted NBT/schematics are per-target build
  outputs (the equivalent of a binary) ([versioning-editions.md](versioning-editions)).

## 1.3 Scope and non-goals
- Redstone **can be described at the logic level** (declare signals, gates, and connections; the
  compiler synthesizes and place-and-routes; see [redstone.md](redstone)). v1 is limited to
  combinational logic plus a curated set of sequential macros; **general sequential synthesis of
  arbitrary state machines / CPUs is out of scope** (drop to Tier 0 physical placement or raw).
- Full NBT recovery (chest contents, command blocks, etc.) is out of scope.
- Importing legacy numeric-ID `.schematic` files (pre-1.13 flattening) is not supported in v1.
