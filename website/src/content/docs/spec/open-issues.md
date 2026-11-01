---
title: "15. Open Issues"
---

## 15.1 Choices to settle at implementation time

**Where provenance lives:** the `.crn` header or the lock. Decided: the `.crn` carries only
`@intended_targets` as a hint and `@cairn` as the language version it was written against, and the
compiler writes records such as `verified` into the lock
([Versioning and Editions §10.6](/spec/versioning-editions/#106-provenance-and-lock)).

**Coordinate system:** keep the corner origin and `front = +z`, or move to a centre origin or an
entrance-relative orientation. Decided: a placement's origin is the low-`x`, low-`z` corner of its
full extent, overhang columns included, which its structure file writes as `(0, 0, 0)`
([Components, Editing, and Multi-building §9.3.2](/spec/components-editing-sites/#932-origin-selectors)).
Coordinates on the member lines of a `struct` or `def` body are measured from the low corner of
the `size=` footprint instead, and shift with `overhang=` as every member does
([Compilation Model §4.3](/spec/compilation/#43-gable-roof-voxel-rules)). Coordinate triples
(`from=1,1,1`, `at=4,3,2`) on those lines are read the same way once the spec defines them. `y`
rises from the floor, and `front` / `back` / `left` / `right` face `+z` / `-z` / `-x` / `+x`
([Syntax §5.4](/spec/syntax/#54-selectors),
[§9.3.1](/spec/components-editing-sites/#931-coordinate-convention)). A centre origin would sit
off the true centre on every even side, behind a tie-break the author has to remember, as
`at=center` takes the column right of an even wall's midpoint. An entrance-relative frame would add
nothing: the author chooses which wall is `front`, and a door's `side=` already names the entrance
wall. Which way a build faces in the world is chosen when a player places its structure file in the
game, since `place` has no rotation, so a fixed frame does not fix the facing. A per-floor local
`y=0` comes from `level y=N`
([Compilation Model §4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)).

**The type of the reverse-direction IR:** one IR that can degenerate, or separate Intent IR and
Raw Geometry IR types. The tentative policy is to share the block-array layer and split types at the
member layer above it ([Architecture](/spec/architecture/)).

**Legacy `.schematic`** from before 1.13 is unsupported in v1. A numeric-ID mapping could be
considered later as an option.

**Language-evolution policy:** a Rust-style, year-based opt-in, or changes announced per release.
Decided: per release, as
[Compatibility Tiers §C.3](/spec/compatibility/#c3-how-a-break-is-communicated) records. The tiers
already bound what a release may break, and a Stable surface's `W_DEPRECATED` window already gives a
file at least one release to move, so an opt-in would add a second bound and keep every past
revision's rules alive in the compiler. "Edition" already names Java or Bedrock, and `@cairn` stays
provenance.

## 15.2 Untouched topics

**Primitive promotion.** Whether columns and arches should become semantic primitives. The
decision rests on experimental data from the [Evaluation Framework](/spec/evaluation/).

**Interiors.** Whether the `inside.front` prefix suffices or a higher-level `room` concept is
needed, and whether furniture can be served by a `def` library.
