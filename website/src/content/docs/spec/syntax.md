---
title: "5. Syntax"
---

## 5.1 Lexical
- One line = one command. `#` begins a line comment.
- The line starts with a command keyword (positional). **All remaining arguments MUST be `key=value`.**
  Positional arguments require remembering argument order and are fragile to order hallucination and
  omission. Keys like `mat=` / `side=` act as attention anchors for an LLM and stabilize generation.
  Prefer deterministic generation over a small token saving.

```
window side=front mat_slot=glass offset=2 y=2 size=2x2 sym=true   # OK
window front G 2 2 2x2                                            # forbidden (positional args)
```

## 5.2 Nesting
Keep nesting shallow (`struct` / `def` / `level` / `room` / `theme` / `site`). Deep nesting increases
LLM generation errors.

## 5.3 Headers (optional declarations)
Metadata MAY be placed in headers rather than in the semantic body:

```
@cairn 2026.06                           # optional. The Cairn language version it was written against (CalVer)
@requires version>=1.20                  # capability floor (optional). Conflict with the inferred value → E_REQUIRES_CONFLICT
@intended_targets ["1.20.4","1.21.4"]    # wish/hint. Not a verification record (the record lives in the lock)
```

- `@cairn` is the **version of the Cairn language itself** (see the README's Versioning). It is a
  **separate axis** from `@requires` / `@intended_targets` (Minecraft versions). It is optional, and
  exists as provenance so a future compiler can parse/warn correctly.
- See [versioning-editions.md](versioning-editions) for `@requires`.
- `@intended_targets` is a hint about "which Minecraft version it was designed for", not a claim of
  being verified. The verified target is recorded only in the lock.

## 5.4 Selectors (P4)
- Wall selectors: `front` (+z) / `back` / `left` / `right`. `offset` runs along the wall; `y` is
  measured from the floor (= 0).
- Inside reference: prefixed, e.g. `inside.front`.
- Blocks, block entities, and entities all use the same selector grammar.

## 5.5 IDs, classes, addresses
- Important members MAY declare `id=`. `class=` groups members.
- Unspecified members are auto-assigned a stable, meaning-based address by the compiler (editing model
  in [components-editing-sites.md](components-editing-sites)).
