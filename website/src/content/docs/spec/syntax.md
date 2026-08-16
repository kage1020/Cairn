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

A bare value on a line that reads none is `E_UNEXPECTED_POSITIONAL`. `connect FROM.PORT to
TO.PORT` is the single form with a reader for positionals, and its shape is checked by
`E_CONNECT_ARITY` instead. The rule catches more than the style above: the parser puts any token
that is not `key=`, `-> binding`, or `[selector]` into the same list, so a dropped `=`
(`walls mat_slot=wall height 3`) lands there too — and a `walls` with no `height=` is not
shortened, it is not built at all.

### 5.1.1 Literals and separators
A size literal is exactly two extents, `WxH`. A run that continues past the second — `2x2x9`, or
`2x2y` — is refused rather than read as a size followed by something else. Read that way, the
something else lands among the positional values, where it is reported as a bare argument the
author never wrote (§5.1), at a column past the literal that produced it — and in a declaration
header, which keeps no positional values, the failure surfaces at the end of the line instead.
Refusing at the literal also reaches the tools that never run the checks, such as the tree-sitter
grammar an editor highlights with.

Commas are **optional separators**, not structure: `mat=[a, b]` and `mat=[a b]` name the same two
items, as do `[side=front, y=2]` and `[side=front y=2]`. The two lists differ in how much
punctuation they tolerate — a `[selector]`'s attribute list skips a comma wherever it finds one, so
`[side=front, , y=2]` parses, while a value list reads at most one between items and refuses
`[a, , b]`. The one place a comma carries meaning is the input list of `assert truth(...)`, where
it separates signals whose count the row width is checked against.

A truth-table row assigns one bit per input signal, so `truth(a, b -> out)` takes rows two bits
wide and `{ 2->0 }` or `{ 0->0 }` are refused: a row the evaluator cannot read looks like coverage
and is worse than no row at all.

Indentation is two spaces per level and opens one level at a time. A width that is not a multiple
of two and a jump of more than one level are different mistakes with different repairs, and are
reported as such.

A UTF-8 byte-order mark at the very start of a file is ignored — it is what a default Windows
editor writes, and it is not part of the text. One anywhere else is an ordinary stray character.

A line ends at `\n`, at `\r\n`, or at a lone `\r`, and the three are the same line break. This is
what an editor means by a line — VS Code and Monaco both split on all three — so a diagnostic's
line number and the line number under the cursor name the same row whichever ending the file uses.
Reported positions are one place: a position points at the text that is wrong, so an error at the
end of a line is reported at the end of *that* line and never at the first column of the next one.
The tree-sitter grammar is the exception, and it is a limitation rather than a rule: the row in a
syntax node comes from the tree-sitter runtime, which advances it on `\n` alone, so a file whose
only terminator is a lone `\r` highlights as one long line even though it parses correctly.

## 5.2 Nesting
Keep nesting shallow (`struct` / `def` / `level` / `theme` / `site`). Deep nesting increases
LLM generation errors. (`room` is not in this list: it is still open — see
[Open Issues](open-issues) — so writing one today is `E_UNKNOWN_KEYWORD`.)

Inside a body, `level y=N` is the only member that groups other members, and only in a `struct` or a
`def`. A `site` body is a flat list of `place` and `connect` rows with no grouping construct at all.
An indented body anywhere else produces no blocks — it is not lowered, not placed, and lays no
walkway — so it is `E_UNSUPPORTED_NESTING` rather than a silent drop. What `y=N` means to each
grouped member, and which roles have no lowering at a non-zero `N`, is
[Compilation Model §4.7](compilation#47-level-grouping-and-volume-derivation).

Which keywords a body accepts follows from the same split. A `struct` / `def` body describes one
building's geometry (`floor`, `walls`, `door`, `window`, `roof`, `stair`, `level`,
`pressure_plate`, `circuit`); a `site` body describes a layout (`place`, `connect`). The keyword
table is global, so writing one in the other body parses and classifies — and then reaches nothing,
because the geometry passes bucket by role and the site passes match `place` / `connect`. That is
`E_MISPLACED_MEMBER`, reported once at the offending row; anything indented under it goes with it.
`logic` and `assert` lines are not members, so the rule does not reach them: a `logic` line is
read by redstone synthesis from either body, and an `assert` is read by nothing yet — the
evaluator is not implemented, which is a gap in the compiler rather than in the placement.

Top-level names are scoped per kind: `theme` / `def` / `struct` / `site` are four namespaces, so one
name may appear once in each. Declaring it twice within one kind is `E_DUPLICATE_ITEM`. For `theme` /
`def` / `struct` the name is what binds, so the first declaration resolves and the repeat would
otherwise be dropped from the build without a signal. Two `site` blocks of one name merge instead:
their places share one `site::NAME::` namespace, so nothing is dropped unless a `place id=` repeats
— but `east_of=` cannot reach across the blocks, so the merge is half a merge and still an error.

## 5.3 Headers (optional declarations)
Metadata MAY be placed in headers rather than in the semantic body:

```
@cairn 2026.06                           # optional. The Cairn language version it was written against (CalVer)
@requires version>=1.20                  # capability floor (optional). Malformed → E_INVALID_REQUIRES
@intended_targets ["1.20.4","1.21.4"]    # wish/hint. Not a verification record (the record lives in the lock)
```

- `@cairn` is the **version of the Cairn language itself** (see the README's Versioning). It is a
  **separate axis** from `@requires` / `@intended_targets` (Minecraft versions). It is optional, and
  exists as provenance so a future compiler can parse/warn correctly.
- See [Versioning and Editions](versioning-editions) for `@requires`. Its expression is the subject
  `version`, the operator `>=`, and a dotted-decimal version, with whitespace optional between the
  three: `version>=1.21` and `version >= 1.21` are one requirement. `>=` is the only operator, since
  a floor is the only constraint that composes by folding to the strictest. Every other expression
  is `E_INVALID_REQUIRES` rather than a line that quietly declares nothing — a floor that evaporates
  is worse than an absent one, because it is still in the file for a reader to believe.
- `@intended_targets` is a hint about "which Minecraft version it was designed for", not a claim of
  being verified. The verified target is recorded only in the lock.
- `@cairn` and `@intended_targets` appear **at most once** per module → `E_DUPLICATE_HEADER`. Each
  answers a question that has one answer, and neither has a consumer in the compiler yet, so nothing
  would decide between two of them. `@requires` is the exception: its floors *compose*, folding to
  the strictest across every line, so repeating it adds a constraint rather than displacing one.

## 5.4 Selectors (P4)
- Wall selectors: `front` (+z) / `back` / `left` / `right`. `offset` runs along the wall; `y` is
  measured from the floor (= 0).
- **`offset` origin.** `offset=0` sits at the wall's *left end* viewed from outside that wall.
  Concretely: `front` and `back` walls anchor at low `x` (front from the +z viewpoint, back
  mirrored along x so a `sym=true` opening looks symmetric from either side of the building);
  `left` and `right` walls anchor at low `z` and mirror analogously. `sym=true` mirrors the
  opening across the wall's midpoint (`mirror_offset = wall_length - offset - size_w`); a
  mirror that overlaps the primary rectangle is rejected with a `W_DEFERRED_MEMBER` and only
  the primary is painted.
- **`at=` door anchors.** A door's wall-local column is taken from one of three named anchors:
  `at=center` picks the column at `wall_length / 2` (round-half-up — odd-length walls have a unique
  geometric centre; even-length walls pick the column to the right of the midpoint so the choice is
  deterministic), `at=left` picks the wall-local axis origin (`u = 0`), and `at=right` picks the
  far corner (`u = wall_length - 1`). The same column resolves both the openings cut and any
  `connect` walkway anchored to this door (§9.3.5). Numeric offsets (`at=N`) are reserved for a
  future extension.
- Inside reference: prefixed, e.g. `inside.front`.
- Blocks, block entities, and entities all use the same selector grammar.

## 5.5 IDs, classes, addresses
- Important members MAY declare `id=`. `class=` groups members.
- Unspecified members are auto-assigned a stable, meaning-based address by the compiler (editing model
  in [Components, Editing, and Multi-building](components-editing-sites) §9.2).
- A `place` row is the exception: its `id=` is required, not auto-assigned, and omitting it is
  `E_INCOMPLETE_PLACE` (§9.3.3). An auto-address derives from parent / role / side / level / offset
  and names nothing outside the body it sits in; a `place`'s `id=` is what `east_of=` and `connect`
  refer to and what its `.nbt` is written under, so an invented one would be a name the author never
  wrote and cannot point at.
