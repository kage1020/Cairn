---
title: "5. Syntax"
---

## 5.1 Lexical

One line is one command, and `#` begins a line comment. The line starts with a command keyword;
**every remaining argument MUST be `key=value`.**

```
window side=front mat_slot=glass offset=2 y=2 size=2x2 sym=true   # OK
window front G 2 2 2x2                                            # forbidden (positional args)
```

Positional arguments would mean remembering argument order, which an LLM hallucinates and omits.
Keys like `mat=` and `side=` act as attention anchors and stabilize generation, which is worth
more than the tokens they cost.

A bare value on a line that reads none is `E_UNEXPECTED_POSITIONAL`. `connect FROM.PORT to TO.PORT`
is the one form with a reader for positionals, and its shape is checked by `E_CONNECT_ARITY`
instead.

The parser puts anything that is not `key=`, `-> binding`, or `[selector]` into the positional list,
so a dropped `=` lands there too. `walls mat_slot=wall height 3` is not a `walls` with a shortened
height. It is not built at all.

### 5.1.1 Literals and separators

A size literal is exactly two extents, `WxH`. A run that continues past the second, such as `2x2x9`
or `2x2y`, is refused at the literal rather than read as a size followed by something else.

Commas are **optional separators**, not structure. `mat=[a, b]` and `mat=[a b]` name the same two
items, as do `[side=front, y=2]` and `[side=front y=2]`. The two list kinds differ in how much
punctuation they tolerate:

- A `[selector]`'s attribute list skips a comma wherever it finds one, so `[side=front, , y=2]`
  parses.
- A value list reads at most one comma between items and refuses `[a, , b]`.

The one place a comma carries meaning is the input list of `assert truth(...)`, where it separates
the signals whose count the row width is checked against. A row assigns one bit per input signal, so
`truth(a, b -> out)` takes rows two bits wide and refuses `{ 2->0 }` or `{ 0->0 }`.

The table around those rows is read the same way:

| Case | Code |
|---|---|
| No rows at all | `E_TRUTH_TABLE_EMPTY` |
| Two rows assign one input combination different outputs | `E_TRUTH_TABLE_CONFLICT` (on the later row) |
| A row repeats an earlier one and agrees with it | `W_TRUTH_TABLE_DUPLICATE_ROW` |
| Some input combinations are unassigned | `W_TRUTH_TABLE_PARTIAL` |

The last two are warnings because the rows that *are* present still assert what they say. A
four-input table is sixteen rows, and an author part way through is not blocked.

Indentation is two spaces per level and opens one level at a time. A width that is not a multiple of
two and a jump of more than one level are different mistakes and are reported as such.

A UTF-8 byte-order mark at the very start of a file is ignored; one anywhere else is an ordinary
stray character.

A line ends at `\n`, at `\r\n`, or at a lone `\r`, and all three are the same line break. VS Code
and Monaco use the same rule, so a diagnostic's line number and the line under the cursor name the
same row. A position always points at the text that is wrong, so an error at the end of a line is
reported there and never at the first column of the next one.

The tree-sitter grammar is a known exception: its runtime advances the row on `\n` alone, so a file
terminated only by lone `\r` highlights as one long line even though it parses correctly.

## 5.2 Nesting

Keep nesting shallow: `struct` / `def` / `level` / `theme` / `site`. Deep nesting increases LLM
generation errors. (`room` is not on this list; it is still open, so writing one today is
`E_UNKNOWN_KEYWORD`. See [Open Issues](open-issues).)

Inside a body, `level y=N` is the only member that groups other members, and only in a `struct` or a
`def`. A `site` body is a flat list of `place` and `connect` rows with no grouping construct at all.
An indented body anywhere else is `E_UNSUPPORTED_NESTING` rather than a silent drop. It lowers to
nothing, places nothing, and lays no walkway.

[Compilation Model §4.7](compilation#47-level-grouping-and-volume-derivation) defines what `y=N`
means to each grouped member.

**Which keywords a body accepts** follows the same split. A `struct` / `def` body describes one
building's geometry: `floor`, `walls`, `door`, `window`, `roof`, `stair`, `level`,
`pressure_plate`, `circuit`. A `site` body describes a layout: `place`, `connect`. The keyword table
is global, so writing one in the other body parses and classifies and then reaches nothing. That is
`E_MISPLACED_MEMBER`, reported once at the offending row, taking anything indented under it along.

`logic` and `assert` lines are not members, so the rule does not reach them. A `logic` line is read
by redstone synthesis from either body, and an `assert` is read by nothing yet.

**Top-level names are scoped per kind.** `theme` / `def` / `struct` / `site` are four namespaces, so
one name may appear once in each. Declaring it twice within one kind is `E_DUPLICATE_ITEM`. For
`theme` / `def` / `struct` the name is what binds, so the first declaration resolves and the repeat
would otherwise vanish without a signal. Two `site` blocks of one name merge instead, sharing one
`site::NAME::` namespace, but `east_of=` cannot reach across the blocks. The merge is half a merge,
and still an error.

## 5.3 Headers

Metadata MAY go in headers rather than in the semantic body:

```
@cairn 2026.06                           # the Cairn language version this file was written against
@requires version>=1.20                  # capability floor on the Minecraft target
@intended_targets ["1.20.4","1.21.4"]    # a hint, not a verification record
```

**`@cairn`** is the version of the Cairn language itself, a separate axis from the two Minecraft
headers. It is optional and exists as provenance, so a future compiler can parse and warn correctly.

**`@requires`** is a capability floor. Its expression is the subject `version`, the operator `>=`,
and a dotted-decimal version, with whitespace optional between the three, so `version>=1.21` and
`version >= 1.21` are one requirement. `>=` is the only operator, since a floor is the only
constraint that composes by folding to the strictest. Every other expression is
`E_INVALID_REQUIRES` rather than a line that quietly declares nothing: a floor that evaporates is
worse than an absent one, because a reader will still believe it. See
[Versioning and Editions](versioning-editions).

**`@intended_targets`** says which Minecraft versions the file was designed for. It is not a claim
of being verified. That record lives only in the lock.

`@cairn` and `@intended_targets` appear at most once per module, and a repeat is
`E_DUPLICATE_HEADER`. `@requires` is the exception: its floors compose, so repeating it adds a
constraint rather than displacing one.

## 5.4 Selectors

Wall selectors are `front` (+z), `back`, `left`, and `right`. `offset` runs along the wall, and `y`
is measured from the floor (`y = 0`). Inside faces are prefixed: `inside.front`. Blocks, block
entities, and entities all use one selector grammar.

**`offset` origin.** `offset=0` sits at the wall's left end viewed from outside that wall. The
`front` and `back` walls anchor at low `x`, `front` from the +z viewpoint and `back` mirrored along
x so a `sym=true` opening looks symmetric from either side. The `left` and `right` walls anchor at
low `z` and mirror the same way.

`sym=true` mirrors the opening across the wall's midpoint
(`mirror_offset = wall_length - offset - size_w`). A mirror overlapping the primary rectangle is
rejected with `W_DEFERRED_MEMBER`, and only the primary is painted.

**`at=` door anchors.** A door's wall-local column comes from one of three named anchors:

| Anchor | Column |
|---|---|
| `at=center` | `wall_length / 2`, rounded half up. Odd walls have a unique centre; even walls pick the column right of the midpoint. |
| `at=left` | The wall-local axis origin, `u = 0`. |
| `at=right` | The far corner, `u = wall_length - 1`. |

The same column resolves both the openings cut and any `connect` walkway anchored to this door ([§9.3.5](components-editing-sites#935-ports-and-connect)).
Numeric offsets (`at=N`) are reserved for a future extension.

## 5.5 IDs, classes, addresses

Important members MAY declare `id=`, and `class=` groups members. Members without an `id=` get a
stable, meaning-based address assigned by the compiler, derived from parent / role / side / level /
offset. See
[Components, Editing, and Multi-building §9.2](components-editing-sites#92-editing-model).

A `place` row is the exception: its `id=` is required, and omitting it is `E_INCOMPLETE_PLACE`. An
auto-address names nothing outside the body it sits in. A `place`'s `id=` is what `east_of=` and
`connect` refer to, and what its `.nbt` is written under, so an invented one would be a name the
author never wrote and cannot point at.
