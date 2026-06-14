# 11. Lint and Constraint Validation

The compiler returns warnings/errors with line numbers. The form and granularity of error reporting are
first-class parts of the spec; messages MUST be in a shape that feeds the self-correction loop
([evaluation.md](evaluation.md)) — "what is wrong / valid candidates in the target / a suggested fix".

## 11.1 Categories
- **Syntax**: parse / types / `key=value` validity.
- **Geometry**: AABB expansion detecting "window outside the wall", "door hanging in mid-air".
- **attachment**: whether a frame/painting/sign/button/lever/torch is on a valid attachment face
  (detect attachment to air).
- **entity_aabb**: armor_stand/villager/display not clipping into walls/paths, not blocking a door's
  swing arc, entity cramming (density).
- **support**: support conditions for hanging lanterns, torches, campfires, and gravity blocks such as
  gravel.
- **fluid**: consistency of water source / flow / waterlogged.
- **version_caps / parity**: whether a state/entity schema is usable in the target
  ([versioning-editions.md](versioning-editions.md)).
- **edit_stability**: whether an `intent_state` change ripples into an unrelated member's
  `resolved_state`.
- **redstone**: simulate the synthesized circuit per tick and check it against the declared truth table
  / temporal assertions; timing conflicts, QC dependence, routing congestion ([redstone.md](redstone.md)).
- **AABB interference**: on overlap, priority-merge or reject with a lint error. Boundary blockstate
  re-resolution (inner-corner stairs, etc.) is the IR layer's responsibility.

## 11.2 Error vs warning
- Things that, left alone, cause unintended results — concept absence, unknown IDs, out-of-domain
  states — are **errors** (silent substitution and implicit dropping are forbidden).
- Semantic drift across versions/editions, the non-guarantee of redstone behavior, etc. are
  **warnings**.
- Whether autofix is offered is defined by the implementation.

## 11.3 Constraint catalog
In-game constraints (gravity blocks, attachment conditions, fluid flow, disallowed attachment
combinations, etc.) are cataloged and managed per version ([versioning-editions.md](versioning-editions.md)).
A constraint such as "a frame cannot hang on glass" lives here.
