---
title: "11. Lint and Constraint Validation"
---

The compiler returns warnings/errors with line numbers. The form and granularity of error reporting are
first-class parts of the spec; messages MUST be in a shape that feeds the self-correction loop
([Evaluation Framework](evaluation)) — "what is wrong / valid candidates in the target / a suggested fix".

## 11.1 Categories
- **Syntax**: parse / types / `key=value` validity. Concrete codes shipped
  in `cairn check` today:
  - `E_DUPLICATE_SIZE` — header has more than one `size=`.
  - `E_DUPLICATE_SLOT` — `theme` body declares the same slot twice.
  - `E_DUPLICATE_SELECTOR` — two selector rows in one `theme` body select
    the same members and bind the same key, so the earlier binding is read
    by nothing (§7.1). Sameness is by meaning, not by source text:
    attribute order does not count, and `class=` / `id=` / `mat_slot=`
    compare as label text, so `small` and `"small"` are one value. Rows
    that bind *different* keys are excluded — they compose, every binding
    reaching every member both rows select — and so are rows whose
    attributes merely overlap, where a member only the wider row selects
    still reads its binding.
  - `E_DUPLICATE_ARG`  — repeated `key=` in the same argument list.
  - `E_DUPLICATE_ID`   — two members share an `id=` within the same
    immediate body scope.
  - `E_DUPLICATE_ITEM` — two top-level items of the same kind share a
    name. The four kinds (`theme` / `def` / `struct` / `site`) are
    separate namespaces, so a name reused across kinds is not a
    collision. For `theme` / `def` / `struct` the name is the binding
    key, so the **first** declaration is the one that resolves and the
    rest bind nothing. For `site` the binding key is the per-place
    `site::NAME::PLACE_ID`, so two blocks of one name merge into a
    shared place namespace instead of shadowing — every place with a
    distinct `id=` still builds, only a repeated `id=` collides, and
    `east_of=` does not reach across the blocks.
  - `E_DUPLICATE_HEADER` — a single-valued `@directive` (`@cairn`,
    `@intended_targets`) is declared more than once (§5.3). `@requires`
    is excluded: its floors compose to the strictest across every line,
    so a second one adds a constraint rather than displacing the first.
  - `E_INVALID_REQUIRES` — an `@requires` expression that is not a version
    floor (§5.3). The accepted shape is `version`, `>=`, and a
    dotted-decimal version, with whitespace optional between the three; the
    code covers an operator other than `>=`, a missing version, a component
    that is not a decimal number or does not fit in a `u32`, and text after
    the version. Reported rather than dropped, because the directive states
    one constraint and an expression that states none leaves a floor in the
    file that no longer reaches the compiler.
  - `E_UNKNOWN_ID` — a resolved block ID the compile's target does not declare
    (`spec/versioning-editions.md` §10.4). Raised during block-array lowering, and only when a
    target is pinned, so `cairn compile --target` is the one command that reports it —
    `cairn check` does not run lowering at all. Covers an ID the author wrote, an ID the
    registry pack's materials catalog produced, and the ID a member default falls back to when
    the pack declares no row for it; `data.origin` says which, because only the first is a fix
    the author makes in their own source.
  - `E_UNKNOWN_KEYWORD` — statement keyword is not in the known-keyword table.
  - `E_MISPLACED_MEMBER` — statement keyword is in the table but the
    enclosing body has no reader for it: a `place` / `connect` in a
    `struct` or `def`, or a geometry keyword among a `site`'s rows
    (§5.2). Reported once at the offending row; anything indented under
    it goes with it, and the note counts those lines.
  - `E_UNEXPECTED_POSITIONAL` — a bare value on a line that reads none
    (§5.1). `connect FROM.PORT to TO.PORT` is the one exception and is
    covered by `E_CONNECT_ARITY`. Anchored on the run from the first
    bare value to the last, which is not necessarily a prefix of the
    line: a dropped `=` leaves bare values after an argument.
  - `E_UNSUPPORTED_NESTING` — a member carries an indented body that
    nothing reads. Only `level y=N` inside a `struct` or `def` groups
    members; a `site` body is a flat list of `place` and `connect` rows.
    Anchored on the run of indented members, reported once per dropped
    subtree at its root.
  - `E_TYPE_MISMATCH_LABEL` — a label-typed key's value is not a label
    (identifier or string). The label-typed keys are `id=`, `class=`,
    `mat_slot=`, `use=`, and `theme=`. For the last two the mistyped
    value is indistinguishable at the resolver from the key being absent;
    both are errors, and this is the one that says the key is on the line
    but unusable (the other is `E_INCOMPLETE_PLACE`).
  - `E_TYPE_MISMATCH_SIZE`  — `size=` value is not a `WxH` literal.
  - `E_CONNECT_ARITY` — `connect` row whose positional shape is not
    `FROM.PORT to TO.PORT`: a half is missing, the literal `to`
    keyword is missing or replaced by another token, extra
    positionals trail `TO.PORT`, or an endpoint slot holds something
    other than a one-dot `PLACE.PORT` reference (a bare identifier, a
    literal, a `@material` token, a quoted string, a list, or a
    reference carrying a second dot). Anchored at the missing-positional
    cursor, the offending separator, the offending endpoint, or the run
    of trailing extras. Each endpoint is reported separately: the two
    ends are independent fix sites.
  - `E_THEME_VARIANT_MISSING` — the module declares a theme, and the pinned edition can bind none
    of its per-edition variants (`spec/versioning-editions.md` §10.7). Only fires under
    `--edition`: with no pin there is nothing a variant fails to satisfy, and the same source is
    accepted. Reported **once per logical theme**, however many scopes and `place ... theme=` rows
    read it — they all ask for the same edit in the same `theme` block. Every placement naming it is
    still refused; what is deduplicated is the sentence, not the consequence. A module that declares
    such a theme and never reads a `mat_slot=` from it is not reported at all: nothing is starved,
    and the build is byte-identical with or without the pin.
  - `E_INCOMPLETE_PLACE` — a `place` row omits `id=`, `use=`, or
    `theme=` (§9.3). The row cannot become a placement without all three,
    so it is dropped from the build; the message names every key the row
    is short of. A key that is *present but mistyped* is
    `E_TYPE_MISMATCH_LABEL` instead — it is on the line, just not a label.
- **Geometry**: AABB expansion detecting "window outside the wall", "door hanging in mid-air".
- **attachment**: whether a frame/painting/sign/button/lever/torch is on a valid attachment face
  (detect attachment to air).
- **entity_aabb**: armor_stand/villager/display not clipping into walls/paths, not blocking a door's
  swing arc, entity cramming (density).
- **support**: support conditions for hanging lanterns, torches, campfires, and gravity blocks such as
  gravel.
- **fluid**: consistency of water source / flow / waterlogged.
- **version_caps / parity**: whether a state/entity schema is usable in the target
  ([Versioning and Editions](versioning-editions)).
- **edit_stability**: whether an `intent_state` change ripples into an unrelated member's
  `resolved_state`.
- **redstone**: simulate the synthesized circuit per tick and check it against the declared truth table
  / temporal assertions; timing conflicts, QC dependence, routing congestion ([Redstone](redstone)).
- **AABB interference**: on overlap, priority-merge or reject with a lint error. Boundary blockstate
  re-resolution (inner-corner stairs, etc.) is the IR layer's responsibility.

Diagnostics that reject an identifier against a closed vocabulary (unknown statement keyword,
unknown `mat_slot=` name, unknown `--target` version) attach a `did you mean \`X\`?` note when a
candidate sits within a length-scaled Damerau-Levenshtein cap (≤ 1 edit for 1–3 char inputs, ≤ 2
for 4–6, ≤ 3 beyond). The closed-set listing (`expected one of: ...`) stays as the fallback so the
output covers both the targeted fix and the full set of valid candidates.

## 11.2 Machine-readable payload

The `--format json` output renders one object per finding with the following shape:

| Field      | Type     | Notes                                                                 |
| ---------- | -------- | --------------------------------------------------------------------- |
| `code`     | string   | Stable `E_*` / `W_*` identifier; same string as the gcc-style format. |
| `severity` | string   | `"error"` or `"warning"`.                                             |
| `line`     | integer  | 1-based line of the primary span's first byte.                        |
| `col`      | integer  | 1-based column of the same byte, counted in Unicode scalar values.    |
| `end_line` | integer  | 1-based line of the span's last-byte-exclusive boundary.              |
| `end_col`  | integer  | 1-based column of the same boundary.                                  |
| `primary`  | string   | Human-readable message printed after the code in the text format.    |
| `notes`    | array    | `[{line?, col?, message}]`. Optional — omitted entirely when empty.   |
| `data`     | object   | Structured payload — see below. Optional — omitted when absent.       |

`data` is an open, code-specific object tagged with `kind`. Consumers that depend on a particular
key set should match on `(code, data.kind)` rather than inspecting `primary`. The shape is
evolving — additions for new codes are strictly additive, so consumers should ignore unknown
`kind` values rather than failing on them. Current entries:

| Code                 | `data` payload                                                   |
| -------------------- | ---------------------------------------------------------------- |
| `W_WALKWAY_BLOCKED`  | `{ "kind": "walkway_blocked", "skipped": <u64> }` — number of cells along the fallback L-shaped path that overlapped an existing structure and were dropped from the lay (emitted only when the detour search found no unobstructed route). |
| `E_DUPLICATE_SELECTOR` | `{ "kind": "duplicate_selector", "rebound": ["frame"] }` — the binding keys this selector row takes over from an earlier one, without the trailing `=`, in the order the message lists them. Always non-empty. |
| `E_UNKNOWN_ID` | `{ "kind": "unknown_id", "id": "minecraft:oak_plank", "registry": "java 1.21.4", "origin": "authored", "suggestion": "minecraft:oak_planks" }` — the ID the pinned target does not declare, the target it was checked against, and who chose it. `origin` is `authored` when the source names the ID, `catalog` when the pack maps a token onto it, and `builtin` when the pack declares no row for a member default and the compiler's own ID was used. `token` accompanies the latter two and is absent for `authored`; `suggestion` is absent when no declared ID is within the typo threshold, which is always the case for a rename. |
| ↳ `origin: "catalog"` | `{ …, "id": "minecraft:stone_bricks", "registry": "bedrock 1.21.0", "origin": "catalog", "token": "floor.stone.smooth", "suggestion": "minecraft:stonebrick" }` — the author's token is correct and the pack's mapping is not, so the edit does not belong in the source. |
| ↳ `origin: "builtin"` | `{ …, "id": "minecraft:oak_pressure_plate", "registry": "bedrock 1.21.60", "origin": "builtin", "token": "pressure_plate.default" }` — the pack declares no row for that member default, so the ID compiled into the compiler was used. The pack is what has to grow the row. |
| `E_INCOMPLETE_PLACE` | `{ "kind": "incomplete_place", "missing": ["id", "use", "theme"] }` — the keys the `place` row does not declare, without the trailing `=`, in the order the message lists them. Always non-empty. |

Codes not listed above omit `data` entirely; reading `entry.data` returns `undefined` and the JSON
key is absent (it does not serialise as `null`). New `data` entries land alongside the code that
needs them as the diagnostic surface stabilises.

## 11.3 Error vs warning
- Things that, left alone, cause unintended results — concept absence, unknown IDs, out-of-domain
  states — are **errors** (silent substitution and implicit dropping are forbidden).
- Semantic drift across versions/editions, the non-guarantee of redstone behavior, etc. are
  **warnings**. So are the partial-build degradations the block-array pass reports (`W_*`), where
  the compiler rather than the source is the incomplete side and `cairn compile` refuses separately.
- The `E_` / `W_` prefix is not the severity. `W_` marks a partial-build degradation, and two
  `E_`-prefixed codes are decided against the rule above rather than by their name:
  `E_UNKNOWN_SLOT_TARGET` is an **error**, because a slot bound to a non-material value lowers every
  member that references it to air; `E_THEME_SELECTOR_UNMATCHED` is a **warning**, because a rule
  that matches nothing overrides nothing and the build is what the rest of the source asked for.
- Whether autofix is offered is defined by the implementation.

## 11.4 Constraint catalog
In-game constraints (gravity blocks, attachment conditions, fluid flow, disallowed attachment
combinations, etc.) are cataloged and managed per version ([Versioning and Editions](versioning-editions)).
A constraint such as "a frame cannot hang on glass" lives here.
