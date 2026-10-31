---
title: "15. Open Issues"
---

## 15.1 Choices to settle at implementation time

**Where provenance lives:** the `.crn` header or the lock. Decided: the `.crn` carries only
`@intended_targets` as a hint and `@cairn` as the language version it was written against, and the
compiler writes records such as `verified` into the lock
([Versioning and Editions §10.6](/spec/versioning-editions/#106-provenance-and-lock)).

**Coordinate system:** keep the corner origin and `front = +z`, or move to a centre origin or an
entrance-relative orientation. Decided: a build's origin is the low corner of its extent, `y` rises
from the floor, and `front` / `back` / `left` / `right` face `+z` / `-z` / `-x` / `+x`
([Syntax §5.4](/spec/syntax/#54-selectors),
[Components, Editing, and Multi-building §9.3.1](/spec/components-editing-sites/#931-coordinate-convention)).
A structure file is placed from its low corner, so the origin is the file's own. An even side has no
centre voxel to hold one. A building is turned when it is placed in the world, and `front` already
names its entrance, so a frame relative to the entrance would add nothing. A per-floor local `y=0`
comes from `level y=N` ([Compilation Model §4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)).
Coordinate triples (`from=1,1,1`, `at=4,3,2`) are read in this frame when they are specified.

**The type of the reverse-direction IR:** one IR that can degenerate, or separate Intent IR and
Raw Geometry IR types. The tentative policy is to share the block-array layer and split types at the
member layer above it ([Architecture](/spec/architecture/)).

**Legacy `.schematic`** from before 1.13 is unsupported in v1. A numeric-ID mapping could be
considered later as an option.

## 15.2 Untouched topics

**Primitive promotion.** Whether columns and arches should become semantic primitives. The
decision rests on experimental data from the [Evaluation Framework](/spec/evaluation/).

**Interiors.** Whether the `inside.front` prefix suffices or a higher-level `room` concept is
needed, and whether furniture can be served by a `def` library.

## 15.3 Language-evolution policy

How to handle breaking changes as Cairn evolves is not settled. The options are a Rust-style
"edition" mechanism with year-based opt-in, or announcing changes per release in the CHANGELOG. Note
that "edition" already means Java or Bedrock here, so a different term would be needed. For now the
latter suffices: CalVer plus `@cairn` provenance.
