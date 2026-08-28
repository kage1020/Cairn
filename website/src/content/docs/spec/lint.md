---
title: "11. Lint and Constraint Validation"
---

The compiler reports warnings and errors with line numbers. Every message MUST carry the
self-correction triple: **what is wrong / valid candidates in the target / a suggested fix**. That
shape is what feeds the loop in [Evaluation Framework](evaluation).

## 11.1 Diagnostic codes

### Duplicates

| Code | Meaning |
|---|---|
| `E_DUPLICATE_SIZE` | A header declares more than one `size=`. |
| `E_DUPLICATE_SLOT` | A `theme` body declares the same slot twice. |
| `E_DUPLICATE_ARG` | A `key=` is repeated in one argument list. |
| `E_DUPLICATE_ID` | Two members share an `id=` in the same body scope. |
| `E_DUPLICATE_SELECTOR` | Two selector rows in one `theme` select the same members and bind the same key. |
| `E_DUPLICATE_ITEM` | Two top-level items of the same kind share a name. |
| `E_DUPLICATE_HEADER` | A single-valued `@directive` is declared more than once. |

`E_DUPLICATE_SELECTOR` compares selectors by meaning, not by text: attribute order does not count,
and `class=` / `id=` / `mat_slot=` compare as label text, so `small` and `"small"` are one value.
Rows that bind *different* keys compose and are not reported. Neither are rows whose attributes
partly overlap (see [Materials and Themes §7.1](materials-themes#71-slots-as-dependency-injection)).

`E_DUPLICATE_ITEM` treats `theme` / `def` / `struct` / `site` as four separate namespaces, so one
name may appear once in each. For the first three the first declaration resolves and the rest bind
nothing. Two `site` blocks of one name instead merge into a shared `site::NAME::PLACE_ID`
namespace: every place with a distinct `id=` still builds, only a repeated `id=` collides, and
`east_of=` does not reach across the blocks.

`E_DUPLICATE_HEADER` covers `@cairn` and `@intended_targets`. `@requires` is excluded: its floors
compose to the strictest across every line, so a second one adds a constraint ([§5.3](syntax#53-headers)).

### Syntax and structure

| Code | Meaning |
|---|---|
| `E_UNKNOWN_KEYWORD` | The statement keyword is not in the known-keyword table. |
| `E_UNKNOWN_ARGUMENT` | A `key=` outside the vocabulary of the member's keyword. |
| `E_MISPLACED_MEMBER` | The keyword is known, but the enclosing body has no reader for it. |
| `E_UNEXPECTED_POSITIONAL` | A bare value on a line that reads none ([§5.1](syntax#51-lexical)). |
| `E_UNSUPPORTED_NESTING` | A member carries an indented body that nothing reads. |
| `E_TYPE_MISMATCH_LABEL` | A label-typed key's value is not an identifier or string. |
| `E_TYPE_MISMATCH_SIZE` | A `size=` value is not a `WxH` literal. |
| `E_CONNECT_ARITY` | A `connect` row's shape is not `FROM.PORT to TO.PORT`. |
| `E_INVALID_REQUIRES` | An `@requires` expression that is not a version floor ([§5.3](syntax#53-headers)). |

`E_MISPLACED_MEMBER` fires on a `place` / `connect` inside a `struct` or `def`, or a geometry
keyword among a `site`'s rows. It is reported once at the offending row, and anything indented
under it goes with it.

`E_UNSUPPORTED_NESTING`: only `level y=N` inside a `struct` or `def` groups members, and a `site`
body is a flat list. Reported once per dropped subtree, at its root.

`E_TYPE_MISMATCH_LABEL`: the label-typed keys are `id=`, `class=`, `mat_slot=`, `use=`, and
`theme=`. For `use=` and `theme=` a mistyped value looks the same as an absent key to the resolver.
This code says the key is on the line but unusable; `E_INCOMPLETE_PLACE` says it is missing.

`E_CONNECT_ARITY`: `connect FROM.PORT to TO.PORT` is the one form that reads positionals. The code
covers a missing half, a missing or replaced `to` keyword, extra trailing positionals, and an
endpoint that is not a one-dot `PLACE.PORT` reference. The two endpoints are reported separately,
since they are independent fix sites.

`E_INVALID_REQUIRES`: the accepted shape is `version`, `>=`, and a dotted-decimal version, with
whitespace optional. The code covers any other operator, a missing version, a component that is not
a decimal number or does not fit in a `u32`, and text after the version.

### Materials and targets

| Code | Meaning |
|---|---|
| `E_UNKNOWN_ID` | A resolved block ID the pinned target does not declare. |
| `E_INCOMPATIBLE_MATERIAL` | A member whose geometry attaches blockstates is bound to a material that cannot carry them. |
| `E_THEME_VARIANT_MISSING` | The pinned edition can bind none of a theme's per-edition variants. |
| `E_INCOMPLETE_PLACE` | A `place` row omits `id=`, `use=`, or `theme=` ([§9.3](components-editing-sites#93-multi-building-with-site)). |

`E_UNKNOWN_ID` and `E_INCOMPATIBLE_MATERIAL` are raised during block-array lowering, so only
`cairn compile` (and `cairn lower`) report them. `cairn check` does not run lowering at all.
`E_UNKNOWN_ID` further needs a pinned target, so `cairn compile --target` is the one command that
raises it. See
[Versioning and Editions §10.4](versioning-editions#104-fail-loud-and-minimum-version-inference).

`E_INCOMPATIBLE_MATERIAL` today means a sloped roof or an eave `stair` bound outside the stair
family ([Compilation Model §4.3](compilation#43-gable-roof-voxel-rules)).

`E_THEME_VARIANT_MISSING` fires only under `--edition`, and is reported **once per logical theme**
however many scopes read it, since they all want the same edit in the same `theme` block. Every
placement naming it is still refused. A module that declares such a theme but never reads a
`mat_slot=` from it is not reported: the build is byte-identical with or without the pin.

`E_INCOMPLETE_PLACE` names every key the row is short of, and the row is dropped from the build.

### Truth tables

| Code | Meaning |
|---|---|
| `E_TRUTH_TABLE_EMPTY` | An `assert truth(...)` with no rows. |
| `E_TRUTH_TABLE_CONFLICT` | Two rows assign the same input combination different outputs. |
| `W_TRUTH_TABLE_DUPLICATE_ROW` | A row repeats an earlier one and agrees with it. |
| `W_TRUTH_TABLE_PARTIAL` | The rows leave input combinations unassigned. |

`E_TRUTH_TABLE_CONFLICT` is reported on the later row, with a note at the first row carrying that
pattern. The spec does not say which of the two an evaluator would read, because the repair is to
decide which row is wrong.

The two warnings are warnings because every row present is still a real constraint. One table can
earn both: a repeated row fills no combination.

### Semantic categories

Beyond the codes above, lint covers:

| Category | Checks |
|---|---|
| **Geometry** | AABB expansion: a window outside the wall, a door hanging in mid-air. |
| **attachment** | A frame, painting, sign, button, lever, or torch on a valid attachment face. |
| **entity_aabb** | Entities not clipping walls or paths, not blocking a door's swing, not cramming. |
| **support** | Hanging lanterns, torches, campfires, and gravity blocks such as gravel. |
| **fluid** | Consistency of water source, flow, and `waterlogged`. |
| **version_caps / parity** | Whether a state or entity schema is usable in the target ([Versioning and Editions](versioning-editions)). |
| **edit_stability** | Whether an `intent_state` change ripples into an unrelated member's `resolved_state`. |
| **redstone** | Per-tick simulation against the declared truth table and temporal assertions; timing conflicts, QC dependence, routing congestion ([Redstone](redstone)). |
| **AABB interference** | On overlap, priority-merge or reject. Boundary blockstate re-resolution is the IR layer's job. |

### "did you mean"

Three diagnostics reject an identifier against a closed vocabulary: an unknown keyword, an unknown
`mat_slot=` name, and an unknown `--target` version. Each attaches a ``did you mean `X`?`` note when
a candidate sits within a length-scaled Damerau-Levenshtein cap of ≤ 1 edit for 1–3 characters, ≤ 2
for 4–6, and ≤ 3 beyond. The closed-set listing (`expected one of: ...`) is always printed as well.

## 11.2 Machine-readable payload

`--format json` renders one object per finding:

| Field | Type | Notes |
|---|---|---|
| `code` | string | Stable `E_*` / `W_*` identifier; same string as the gcc-style format. |
| `severity` | string | `"error"` or `"warning"`. |
| `line` | integer | 1-based line of the primary span's first byte. |
| `col` | integer | 1-based column of the same byte, in Unicode scalar values. |
| `end_line` | integer | 1-based line of the span's exclusive end boundary. |
| `end_col` | integer | 1-based column of the same boundary. |
| `primary` | string | The human-readable message. |
| `notes` | array | `[{line?, col?, message}]`. Omitted when empty. |
| `data` | object | Code-specific payload. Omitted when absent. |

`data` is an open object tagged with `kind`. Match on `(code, data.kind)` rather than parsing
`primary`. Additions are strictly additive, so ignore unknown `kind` values rather than failing on
them. Codes not listed below omit `data` entirely, so the JSON key is absent rather than `null`.

| Code | `data` payload |
|---|---|
| `W_WALKWAY_BLOCKED` | `{ "kind": "walkway_blocked", "skipped": <u64> }`. Cells along the fallback L-shaped path that overlapped an existing structure and were dropped. |
| `E_DUPLICATE_SELECTOR` | `{ "kind": "duplicate_selector", "rebound": ["frame"] }`. The binding keys this row takes over from an earlier one, without the trailing `=`. Never empty. |
| `E_UNKNOWN_ID` | `{ "kind": "unknown_id", "id", "registry", "origin", "token"?, "suggestion"? }`. See below. |
| `E_INCOMPATIBLE_MATERIAL` | `{ "kind": "incompatible_material", "id", "required", "slot"?, "token"? }`. The bound material, the family the geometry needs, and where the binding came from. |
| `E_INCOMPLETE_PLACE` | `{ "kind": "incomplete_place", "missing": ["id", "use", "theme"] }`. The keys the row does not declare. Never empty. |
| `E_INVALID_REQUIRES` | `{ "kind": "invalid_requires", "reason", "found" }`. `reason` is one of `not_a_version_requirement`, `unsupported_operator`, `empty_version`, `component_not_a_number`, `component_too_large`, `trailing_tokens`. `found` is empty when the failure names no fragment. |
| `W_TRUTH_TABLE_PARTIAL` | `{ "kind": "truth_table_partial", "inputs": 2, "covered": 1, "missing": ["01","10","11"] }`. See below. |

**`E_UNKNOWN_ID.origin`** says who chose the ID, because the repair differs:

| `origin` | Meaning | Where the fix goes |
|---|---|---|
| `authored` | The source names the ID. | The author's line. |
| `catalog` | The registry pack maps a token onto it. | The pack's mapping. |
| `builtin` | The pack declares no row for a member default, so the compiler's own ID was used. | The pack, which has to grow the row. |

`token` accompanies `catalog` and `builtin` and is absent for `authored`. `suggestion` is absent
when no declared ID is within the typo threshold, which is always the case for a rename.

`E_INCOMPATIBLE_MATERIAL` follows the same idea: `slot` is the `mat_slot=` name the member read and
is absent when it carries no binding, and a dotted `token` (`roof.dark_wood`) means the pack's
mapping is what to correct rather than the source line. `required` is named rather than implied so
that adding a second family later is a new value here, not a new code.

`W_TRUTH_TABLE_PARTIAL.missing` is a **sample** rather than the set: twenty inputs have a million
combinations. Take the count from `2^inputs - covered`, never from `missing.len()`. `inputs` is
carried instead of that total because the grammar puts no ceiling on the input list and no integer
holds `2^130`.

## 11.3 Error vs warning

- **Errors** are things that, left alone, produce unintended results: concept absence, unknown IDs,
  out-of-domain states. Silent substitution and implicit dropping are forbidden.
- **Warnings** are semantic drift across versions and editions, the non-guarantee of redstone
  behaviour, and the partial-build degradations the block-array pass reports. In those the compiler
  rather than the source is the incomplete side.

The `E_` / `W_` prefix is not the severity. `W_` marks a partial-build degradation, and two
`E_`-prefixed codes are decided by the rule above rather than by their name:

- `E_UNKNOWN_SLOT_TARGET` is an **error**, because a slot bound to a non-material value lowers
  every member referencing it to air.
- `E_THEME_SELECTOR_UNMATCHED` is a **warning**, because a rule that matches nothing overrides
  nothing.

`E_UNKNOWN_ARGUMENT` is an **error** for the same reason `E_UNKNOWN_KEYWORD` is, one level down.
A key outside the keyword's vocabulary names nothing, so no pass will read the value however the
compiler grows, and the member is built without whatever was being asked for. A misspelled argument
that has a default is the worst of them: the build succeeds, at the default, and says nothing.

Each keyword's vocabulary is closed, and a `theme` selector widens the one it names — writing
`window[tags=...]` in a theme makes `tags=` a key something reads on a window, and on nothing else.
The reverse direction is `E_THEME_SELECTOR_UNMATCHED`. A selector coins words; one edit away from a
word the keyword already has is a typo written twice rather than a coinage, and is refused with the
suggestion.

`W_IGNORED_ARGUMENT` is a **warning**, and covers two things. A `key=` in the vocabulary whose value
the pass cannot read is dropped and a default put in its place; and a `key=` this specification
defines that no pass reads yet — `window shape=` / `anchor=` and `roof footprint=` / `bounds=` are
those keys today — is carried into the IR and never consulted. The boundary is the keyword: a
spec-defined key on a keyword the compiler knows is reported this way, while a spec-defined
*keyword* it does not know is `E_UNKNOWN_KEYWORD` and its arguments are not judged at all. Both make the build differ from the source. The rule forbids *silent*
substitution, and both are announced. In the second case the gap is the compiler's rather than the
source's, which is why it is not a refusal. Whether autofix is offered is up to the implementation.

## 11.4 Constraint catalog

In-game constraints are cataloged and managed per version: gravity blocks, attachment conditions,
fluid flow, and disallowed attachment combinations ([Versioning and Editions](versioning-editions)).
"A frame cannot hang on glass" lives there.
