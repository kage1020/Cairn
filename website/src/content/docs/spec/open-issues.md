---
title: "15. Open Issues"
---

## 15.1 Choices to settle at implementation time

**Where provenance lives:** the `.crn` header or the lock. The tentative policy is that the `.crn`
carries only `@intended_targets` as a hint, and records such as `verified` are written into the lock
by the compiler ([Versioning and Editions](versioning-editions)).

**The type of the reverse-direction IR:** one IR that can degenerate, or separate Intent IR and
Raw Geometry IR types. The tentative policy is to share the block-array layer and split types at the
member layer above it ([Architecture](architecture)).

**Legacy `.schematic`** from before 1.13 is unsupported in v1. A numeric-ID mapping could be
considered later as an option.

## 15.2 Untouched topics

**Coordinate system.** Keep the corner origin and `front = +z` fixed, or introduce a centre origin,
an entrance-relative orientation, or a per-floor local `y=0` such as `level id=floor2 y=4`.

**Primitive promotion.** Whether hip, flat, and pyramid roofs, columns, arches, and `repeat` should
become semantic primitives. The decision rests on experimental data from the
[Evaluation Framework](evaluation).

**Interiors.** Whether the `inside.front` prefix suffices or a higher-level `room` concept is
needed, and whether furniture can be served by a `def` library.

## 15.3 Language-evolution policy

How to handle breaking changes as Cairn evolves is not settled. The options are a Rust-style
"edition" mechanism with year-based opt-in, or announcing changes per release in the CHANGELOG. Note
that "edition" already means Java or Bedrock here, so a different term would be needed. For now the
latter suffices: CalVer plus `@cairn` provenance.
