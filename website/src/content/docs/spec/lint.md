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
  - `E_DUPLICATE_ARG`  — repeated `key=` in the same argument list.
  - `E_DUPLICATE_ID`   — two members share an `id=` within the same
    immediate body scope.
  - `E_UNKNOWN_KEYWORD` — statement keyword is not in the known-keyword table.
  - `E_TYPE_MISMATCH_LABEL` — `id=` / `class=` / `mat_slot=` value is not
    a label (identifier or string).
  - `E_TYPE_MISMATCH_SIZE`  — `size=` value is not a `WxH` literal.
  - `E_CONNECT_ARITY` — `connect` row whose positional shape is not
    `FROM.PORT to TO.PORT`: a half is missing, the literal `to`
    keyword is missing or replaced by another token, or extra
    positionals trail `TO.PORT`. Anchored at the missing-positional
    cursor, the offending separator, or the run of trailing extras.
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
| `W_WALKWAY_BLOCKED`  | `{ "kind": "walkway_blocked", "skipped": <u64> }` — number of cells along the L-shaped path that overlapped an existing structure and were dropped from the lay. |

Codes not listed above omit `data` entirely; reading `entry.data` returns `undefined` and the JSON
key is absent (it does not serialise as `null`). New `data` entries land alongside the code that
needs them as the diagnostic surface stabilises.

## 11.3 Error vs warning
- Things that, left alone, cause unintended results — concept absence, unknown IDs, out-of-domain
  states — are **errors** (silent substitution and implicit dropping are forbidden).
- Semantic drift across versions/editions, the non-guarantee of redstone behavior, etc. are
  **warnings**.
- Whether autofix is offered is defined by the implementation.

## 11.4 Constraint catalog
In-game constraints (gravity blocks, attachment conditions, fluid flow, disallowed attachment
combinations, etc.) are cataloged and managed per version ([Versioning and Editions](versioning-editions)).
A constraint such as "a frame cannot hang on glass" lives here.
