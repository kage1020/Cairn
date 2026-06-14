---
title: "15. Open Issues"
---

## 15.1 Design choices to settle at implementation time
- **Where provenance lives**: `.crn` header vs lock. The tentative policy is "the `.crn` carries only
  `@intended_targets` (a hint); records such as `verified` are written into the lock by the compiler"
  ([Versioning and Editions](versioning-editions)).
- **The type of the reverse-direction IR**: a single degenerate-able IR, or separate Intent IR and Raw
  Geometry IR types. The tentative policy is "share the block-array layer; split types at the member
  layer above it" ([Architecture](architecture)).
- **Legacy `.schematic` (pre-1.13)**: unsupported in v1. A numeric-ID mapping could be considered later
  as an option.

## 15.2 Untouched topics
- **Coordinate system**: keep corner origin + front=+z fixed, or introduce a center origin / an
  entrance-relative orientation / a per-floor local y=0 (`level id=floor2 y=4`).
- **Primitive promotion**: whether to promote hip/flat/pyramid roofs, column, arch, repeat, etc. to
  semantic primitives. The decision is based on experimental data from the evaluation framework
  ([Evaluation Framework](evaluation)).
- **Interiors**: whether the `inside.front` prefix suffices or a `room` higher-level concept should be
  introduced; whether furniture can be served by a `def` library.

## 15.3 Language-evolution policy (date-based versioning)
How to handle breaking changes as Cairn evolves is not yet settled: a Rust-style "edition" mechanism
(year-based opt-in) or simply announcing changes per release in the CHANGELOG. Note that the word
"edition" is already used for Java/Bedrock, so a different term would be needed. For now the latter
(CalVer + `@cairn` provenance) suffices.
