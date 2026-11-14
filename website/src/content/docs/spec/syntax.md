---
title: "5. Syntax"
---

## 5.1 Lexical

One line is one command, and `#` begins a line comment. The line starts with a command keyword;
**every remaining argument MUST be `key=value`.** A truth table's braces are the one place a command
may run onto the lines below it ([Literals and separators](#511-literals-and-separators)).

```
window side=front mat_slot=glass offset=2 y=2 size=2x2 sym=true   # OK
window front G 2 2 2x2                                            # forbidden (positional args)
```

Positional arguments would mean remembering argument order, which an LLM hallucinates and omits.
Keys like `mat_slot=` and `side=` act as attention anchors and stabilize generation, which is worth
more than the tokens they cost.

A bare value on a line that reads none is `E_UNEXPECTED_POSITIONAL`. `connect FROM.PORT to TO.PORT`
is the one form with a reader for positionals, and its shape is checked by `E_CONNECT_ARITY`
instead.

The parser puts anything that is not `key=`, `-> binding`, or `[selector]` into the positional list,
so a dropped `=` lands there too. `walls mat_slot=wall height 3` is not a `walls` with a shortened
height. It is not built at all.

A header, a `theme` rule and a `logic` or `assert` line keep one too, and the parse goes on past it.
A `struct` or `def` header and a `theme` selector rule keep their bare values beside their
`key=value` arguments, so `struct s size 3x3` is `E_UNEXPECTED_POSITIONAL` on `size 3x3`, fixed as
`size=3x3`. A `site` or `theme` header reads nothing after its name, a `slot` row nothing after the
value it binds, and an `assert` line nothing after the end of its form: a bare value there is the
same finding, and a `key=value` there is `E_PARSE`. A `logic` line whose `=` is dropped,
`logic sig.a sig.b or sig.c`, still reads the expression after its name, binds nothing, and is
`E_UNEXPECTED_POSITIONAL` on the whole line.

### 5.1.1 Literals and separators

No `key=` value starts with `-`, so a negative number does not parse: `offset=-1` and `scale=-1.5`
are `E_PARSE`. An **integer** is a run of decimal digits, up to 2^63 − 1, or 2^32 − 1 for the
`within` bound of an `assert always`, the `<=` bound of an `assert latency` and the stage count of
a macro such as `pulse(sig.ring, 4)`, which is at least 1 ([Redstone
§14.1](/spec/redstone/#141-two-tiers-and-the-v1-boundary)).

A **decimal** is two runs of decimal digits joined by `.`, as in `scale=1.5`. It has no exponent
and no type suffix, so a run that continues past its digits, such as `1.5e3`, `1.5f` or `1.5.2`, is
refused at the literal, as a size's is. A `.` with no digit after it, `1.`, or none before it, `.5`,
is refused too, and the finding names `1.0` and `0.5`. A decimal stands where an integer stands as
a value: after a key's `=`, in a value list and as a bare value. It is not an integer, so a
coordinate triple's component, a size's extent, a stage count, an assertion's bound and a value in
a block-state literal each refuse one at the literal. The parser keeps it as written, digit for
digit. The key that reads one says whether the writer stores it as a float or a double, and the
writer rounds the written value to the nearest one of that type. Such a key reads an integer as the
same number, so `scale=2` and `scale=2.0` are one value.

No key reads a decimal yet: the keys that will, such as the `scale=` of a display entity
([Entities §8.1](/spec/entities/#81-two-tiers)), belong to keywords the compiler does not know. A
key that reads another kind of value treats a decimal as it treats a coordinate triple (below): a
label or size key refuses it as a type mismatch, a member key such as `height=` or `offset=` refuses
its member as `E_INVALID_MEMBER`, and a key that falls back to its default, such as `sym=` or
`overhang=`, says so with `W_IGNORED_ARGUMENT`.

An **NBT compound** is raw NBT written as SNBT in braces, as in `nbt={variant:"black"}`. It is what
a key reads when a `{` follows its `=`, with a space between them or none. Anywhere else a `{` keeps
its own meaning, so a bare `{a:1}` and a compound in a value list are `E_PARSE`. A compound closes
on the line it opens on, since only a truth table's braces run onto the lines below, and one left
open at the end of its line is `E_PARSE` there. What it holds is the part of Java's SNBT that every
version Cairn targets reads alike:

- A compound is `{}`, or entries `key:value` with exactly one `,` between two of them. A key is
  quoted, or a run of letters, digits, `_`, `-`, `.` and `+`, and a compound names each key once.
- A list is `[]`, or values with exactly one `,` between two of them, all of one type. An array
  opens with `[B;`, `[I;` or `[L;`, nothing between the three characters, and holds bytes, ints or
  longs.
- A number is an optional sign, digits, an optional `.` and digits, and an optional suffix, in
  either case: `b`, `s` and `l` for a byte, a short and a long, `f` and `d` for a float and a
  double. With no suffix it is an int, or a double when it has a `.`. A byte, a short, an int or a
  long has no `.` and no leading zero, and every number is inside its type's range.
- A string is quoted with `"` or `'`, and escapes only `\` and its own quote. `true` and `false` are
  the bytes `1b` and `0b`, and any other unquoted word is refused, so a value is a string only
  where it is quoted: `variant:black` is written `variant:"black"`.
- Compounds, lists and arrays nest at most 64 deep, the outermost compound included.

Anything else is `E_PARSE` where it starts, an exponent, a hexadecimal or binary number, a `_`
between digits, an escape such as `\n`, a function such as `bool(...)` and a comment among them. The
parser keeps the compound as written, braces included, and checks no field in it against the
target: what its fields mean, and the `@edition` guard a line that writes one stands under, are
[Entities §8.1](/spec/entities/#81-two-tiers)'s. No keyword reads `nbt=` yet, so it is a key like
any other that its line does not read: `E_UNKNOWN_ARGUMENT` on a header or a member, and
`W_IGNORED_ARGUMENT` as a theme selector's binding. A key that reads another kind of value treats a
compound as it treats a decimal.

A size literal is exactly two non-zero extents, `WxH`. A run that continues past the second, such
as `2x2x9` or `2x2y`, is refused at the literal rather than read as a size followed by something
else.

A **coordinate triple** is three integers joined by `,` with nothing between them, `x,y,z`, as in
`at=4,3,2` or `from=1,1,1`. It names one cell of the scope it is written in, in the frame the
scope's member lines are measured in ([Open Issues](/spec/open-issues/)): `x` along the `size=`
footprint's `W` from its low-`x` corner, `z` along its `H` from its low-`z` corner, and `y` up from
the base plane, or from the `level`'s under `level y=N`. A cell named this way shifts with a roof's
`overhang=`, as every member does. No component is negative, since none counts back past the low
corner, and each is read under the same ceiling as any integer. A key a later chapter defines as a
distance rather than a cell, such as the `step=` of `raw_repeat`
([Ecosystem Interop](/spec/ecosystem-interop/)), reads the same three integers as the distance from
one repeat to the next.

A triple is a `key=` value: it is what a key reads when an integer, a `,` and another integer
follow its `=`. A `-` before either of those integers still starts a triple, which is then refused
at the `-`, so `from=-1,2,3` and `from=1,-2,3` are told why no component is negative. Anywhere else
the commas keep their own meaning. In a value list they separate
items, so `[1,2,3]` is three integers, and a bare value or a `->` binding reads one integer, so
`fill 1,1,1` is `E_PARSE` at its first `,`. In a selector a `,` followed by a key still separates
attributes: `[y=2,side=front]` is two of them, and `[at=1,2,3, y=2]` a triple and an attribute. A
space on either side of a `,` inside the triple, two components alone, a fourth component, and a
`-` before a component are each refused at the literal.

A cell outside the footprint, or above the top row the scope as written reaches ([Compilation Model
§4.7](/spec/compilation/#47-level-grouping-and-volume-derivation)), is outside the scope. The member
that reads the triple refuses it as `E_INVALID_MEMBER`, as one refuses an `offset=` at or past its
wall's length. The `from=` and `to=` of a `fill`
([Blockstate Model §6.3](/spec/blockstate/#63-waterlogged)) and a `raw_fill`, and a `raw_block`'s
`at=` ([Materials and Themes §7.4](/spec/materials-themes/#74-raw-blocks)), read a triple, and no
other key does yet: `note_block` and the entities that take one are not keywords. A key that reads
another kind of value treats a triple as any value of the wrong kind. A label or size key refuses
it as a type mismatch, a member key such as `side=` or `height=` refuses its member as
`E_INVALID_MEMBER`, and a key that falls back to its default, such as `sym=` or `overhang=`, says
so with `W_IGNORED_ARGUMENT`.

Outside a coordinate triple, commas are **optional separators**, not structure.
`@intended_targets ["1.20.4", "1.21.4"]` and `@intended_targets ["1.20.4" "1.21.4"]` name the same
two versions, as do `[side=front, y=2]` and `[side=front y=2]`. The two list kinds differ in how
much punctuation they tolerate:

- A `[selector]`'s attribute list skips a comma wherever it finds one, so `[side=front, , y=2]`
  parses.
- A value list reads at most one comma between items and refuses `[a, , b]`.

A canonical token may carry a **block-state literal**, as in `@oak_log[axis=x]` or
`@oak_stairs[half=top, facing=north]`. The `[` must touch the token. After a space it is whatever
comes next, so in a value list `[@a [b]]` is still a token and a nested list, while `[@a[b]]` is
refused because `b` is not a property. Each pair inside is `property=value`, where the value is a
word, a run of digits, or `true` / `false`. This is Minecraft's own block-state syntax rather than a
Cairn list, so exactly one comma separates two pairs. An empty literal, a trailing or doubled comma,
and a property named twice are refused. A dotted token such as `@floor.wood` is abstract and takes
no literal, because the theme that binds it chooses the block, so a `[` touching it is whatever
comes next, as one after a space is.

Reading a touching `[` as the literal refuses no source that could build otherwise. Read as whatever
comes next, `mat_slot=@a[1]` would leave a bare value on a member line that reads none, which is
`E_UNEXPECTED_POSITIONAL`, and `mat_slot=[@a[b]]` would put a list where a label belongs, which is
`E_TYPE_MISMATCH_LABEL`.

A **block id** is a namespace and a path joined by `:` with nothing between them, `namespace:path`,
as in `mat=minecraft:stone`, each half an identifier. It names a block as Minecraft spells it,
rather than a meaning a theme binds, and only the `mat=` of a raw member reads one ([Materials and
Themes §7.4](/spec/materials-themes/#74-raw-blocks)). It takes the block-state literal a canonical
token takes, touching it, as in `minecraft:light[level=15]`. Like a triple, it is a `key=` value.
Anywhere else the `:` keeps its own meaning, so the theme rule `slot wall -> minecraft:stone` is
`E_PARSE`: a theme binds canonical tokens ([Materials and Themes
§7.2](/spec/materials-themes/#72-canonical-vocabulary)). A space on either side of the `:` is
`E_PARSE` too, and a key that reads a label, a token or an integer refuses a block id as it refuses
any value of the wrong kind.

A build that pins a version checks the literal's properties and values against the states its
block takes there, so `@oak_log[axis=q]` is `E_STATE_DOMAIN` ([Lint](/spec/lint/)), naming `x`, `y`
and `z` ([Versioning and
Editions](/spec/versioning-editions/#a-state-is-checked-against-its-blocks-states-at-the-target)).

Besides that literal and a coordinate triple, the one place a comma carries meaning is the input
list of `assert truth(...)`, where it separates the signals whose count the row width is checked
against. A row writes one character per input signal (`0`, `1`, or `-`), so `truth(a, b -> out)`
takes rows two characters wide and refuses `{ 2->0 }` or `{ 0->0 }`.

`-` is a **don't-care**: the row means every value of that input, so `0- -> 1` says what `00->1` and
`01->1` say together. It is a shorthand for those rows and not a construct of its own, which is why
two rows may not both stand for one combination (see the table below).

`-` and `->` share a character, and the lexer takes the arrow whenever it can. A row whose last
input is a don't-care is therefore written `11--> 0` or `11- -> 0`; both are the same three-wide
row. Whitespace ends a pattern, so `0- 1 -> 1` is a two-wide pattern and a stray `1`, not a
three-wide row.

A row's **output** is `0`, `1`, or `-` as well, and there it means something else: the row's
combinations are deliberately unconstrained. `--0 -> -` says the table has nothing to say about any
combination with a low third input, which is what answers `W_TRUTH_TABLE_PARTIAL` without asserting
four outputs the author does not mean. The arrow is already read by then, so `-> -` and `->-` are
the same row. A table every row of which has a `-` output constrains nothing and is
`E_TRUTH_TABLE_EMPTY`, the same as a table with no rows.

The table around those rows is read the same way:

| Case | Code |
|---|---|
| One signal listed as two or more inputs | `E_TRUTH_TABLE_DUPLICATE_INPUT` (and nothing else for the table until each signal is listed once) |
| No rows at all, or no row with a `0` or `1` output | `E_TRUTH_TABLE_EMPTY` |
| Two rows assign one input combination different outputs | `E_TRUTH_TABLE_CONFLICT` (on the later row) |
| Two rows cover one input combination without contradicting each other | `W_TRUTH_TABLE_DUPLICATE_ROW` (on the later row) |
| Some input combinations are unassigned | `W_TRUTH_TABLE_PARTIAL` |

The last two are warnings because the rows that *are* present still assert what they say. A
four-input table is sixteen rows, and an author part way through is not blocked.

Between the braces a line break may come before or after any row, after a row's `;`, and before the
`}`, so a table can sit on its `assert` line or give each row a line of its own:

```
assert truth(sig.a, sig.b, sig.enable -> sig.out) {
  001 -> 0
  011 -> 1
  101 -> 1
  111 -> 1
}
```

Inside the braces a line carries no indentation: the rows and the closing `}` may sit at any depth,
and blank and comment lines between them are skipped. A tab in front of a row or the `}` is still
refused, as it is in front of a command. A row is still one line, so a break between its pattern,
its `->` and its output is refused. The `{` stays on the `assert` line, and a `;` ends the row in
front of it on that row's line, so no line may open with one. A table whose `}` is missing is
refused at whatever follows its last row, such as the next command, or at the end of that row's line
when nothing does.

Indentation is two spaces per level and opens one level at a time. A width that is not a multiple of
two and a jump of more than one level are different mistakes and are reported as such. Neither can
be made inside a truth table's braces, whose lines carry no indentation.

Spaces before a line break are not part of the line, so a row may end in them, and a line holding
nothing but spaces is a blank line. A blank line's leading spaces are counted like any other line's
and then discarded with the line, so their width is neither an indent nor a mistake.

A UTF-8 byte-order mark at the very start of a file is ignored; one anywhere else is an ordinary
stray character.

A line ends at `\n`, at `\r\n`, or at a lone `\r`, and all three are the same line break. VS Code
and Monaco use the same rule, so a diagnostic's line number and the line under the cursor name the
same row. A position always points at the text that is wrong, so an error at the end of a line is
reported there and never at the first column of the next one.

The tree-sitter grammar is a known exception in three ways. Its runtime advances the row on `\n`
alone, so a file terminated only by lone `\r` highlights as one long line even though it parses
correctly. It also accepts fourteen forms the parser refuses, because tree-sitter cannot express
their rules: a size with a zero extent (`0x3`), a third extent (`2x2x9`) or a trailing word
(`2x2y`), an integer past 2^63 − 1, alone or as a coordinate's component, an assertion's `within`
or `<=` bound or a macro's stage count past 2^32 − 1, a stage count of 0, a coordinate whose last
component runs into a size (`1,2,3x4`), a truth row whose width differs from the input count, a
block-state literal naming one property twice, a decimal that runs into a word (`1.5e3`), and an
NBT compound that names one key twice, holds a list of mixed types or a number past its type's
range, or nests past 64. And it refuses a bare value after the name on a `site` or
`theme` header, which the parser reads and reports as `E_UNEXPECTED_POSITIONAL`, because taking one
there would let error recovery read an over-indented first body line into the header.

## 5.2 Nesting

Keep nesting shallow: `struct` / `def` / `level` / `theme` / `site`. Deep nesting increases LLM
generation errors. (`room` is not on this list; it is still open, so writing one today is
`E_UNKNOWN_KEYWORD`. See [Open Issues](/spec/open-issues/).)

Inside a body, `level y=N` is the only member that groups other members, and only in a `struct` or a
`def`. A `site` body is a flat list of `place` and `connect` rows with no grouping construct at all.
An indented body anywhere else is `E_UNSUPPORTED_NESTING` rather than a silent drop. It lowers to
nothing, places nothing, and lays no walkway.

That rule is about members, and reaches only bodies that hold them. A `theme` body holds rules,
which bind materials and open nothing, so a line indented under one is a syntax error and not a
nesting diagnostic. So is a line indented after a directive: a directive is one line, and the line
under it belongs to no construct.

An `@edition` guard indents the lines under it too, but it is not a member and groups nothing. Its
lines stand in the body around it, and it holds only raw members and Tier 0 parts ([Versioning and
Editions §10.7](/spec/versioning-editions/#an-edition-guard-holds-the-escape-hatch)).

[Compilation Model §4.7](/spec/compilation/#47-level-grouping-and-volume-derivation) defines what
`y=N` means to each grouped member.

**Which keywords a body accepts** follows the same split. A `struct` / `def` body describes one
building's geometry: `floor`, `walls`, `fill`, `door`, `window`, `roof`, `stair`, `level`,
`pressure_plate`, `lever`, `button`, `daylight`, `observer`, `lamp`, `piston`, `dispenser`, `chest`,
`repeater`, `comparator`, `redstone_torch`, `redstone_dust`, `circuit`, `raw_block`, `raw_fill`. A
`site` body describes a layout: `place`, `connect`. The keyword table is global, so writing one in
the other body parses and classifies and then reaches nothing. That is `E_MISPLACED_MEMBER`,
reported once at the offending row, taking anything indented under it along. A `level` groups the
other members of a `struct` / `def` body, but not a `circuit` line, whose reservation spans the
whole scope: a `circuit` line under a `level` reaches nothing either, and is `E_MISPLACED_MEMBER` on
that line ([Redstone §14.5](/spec/redstone/#145-place-and-route)).

`logic` and `assert` lines are not members, so the rule does not reach them. A `logic` line is read
by redstone synthesis from either body. An `assert` is checked for its own shape and its names:
`cairn check` checks a truth table ([Literals and separators](#511-literals-and-separators)), and
redstone synthesis resolves the signals it names (`E_LOGIC_UNBOUND_SIGNAL`). A latency bound is
counted along the laid circuit, and `truth` and `always` are checked by running it ([Redstone
§14.7](/spec/redstone/#147-verification)).

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
No pass branches on the value, but it is read: `YYYY.M` or `YYYY.M.PATCH`, with a four-digit year
and a month `1 … 12`. A leading zero on the month is accepted, so `2026.06` and `2026.6` are one
version. Anything else is `W_INVALID_CAIRN_VERSION`, and a version later than the compiler reading
it is `W_FUTURE_CAIRN_VERSION`: provenance that a later compiler cannot read is not doing the job
the header exists for.

**`@requires`** is a capability floor. Its expression is an optional edition, the subject
`version`, the operator `>=`, and a version label. Whitespace is optional around the operator, so
`version>=1.21` and `version >= 1.21` are one requirement, and required after an edition:
`javaversion>=1.21` is `E_INVALID_REQUIRES`. `>=` is the only operator, since a floor is the only
constraint that composes by folding to the strictest. Every other expression is
`E_INVALID_REQUIRES` rather than a line that quietly declares nothing: a floor that evaporates is
worse than an absent one, because a reader will still believe it.

```
@requires version>=1.21                  # a floor on whichever edition is built
@requires java version>=1.21.4           # a floor in Java's numbering, inert on a Bedrock build
@requires bedrock version>=1.21.40
```

The edition is there because Java releases run `1.20.4 / 1.21 / 1.21.4 / 26.3` and Bedrock
`1.21.0 / 1.21.40 / 1.21.60 / 1.26.60`: `1.21.4` is a Java release and names no Bedrock release at
all. A label is dot-separated components that each begin with a digit and carry only letters and
digits, with an optional `-` and a pre-release tag of dot-separated letters and digits. `1.21.4`,
`1.21.4-rc1`, and `24w14a` are all labels. Which of them the *target edition* can order is not a
syntax question and is not answered here. See [Versioning and Editions](/spec/versioning-editions/).

**`@intended_targets`** says which Minecraft versions the file was designed for. It is not a claim
of being verified. That record lives only in the lock.

A hint is still weighed against the floor beside it. Take the versions a file's
`@intended_targets` names that the target edition can build. If its `@requires` refuses every one
of them, the file states an intention the compiler will refuse the moment anyone acts on it, and
that is `E_INTENDED_TARGET_CAP`. A list only partly below the floor is `W_INTENDED_TARGET_CAP`, and
a version the target edition cannot build at all is `W_INTENDED_TARGET_UNSUPPORTED`. See
[Versioning and Editions §10.4](/spec/versioning-editions/#the-hint-is-weighed-against-the-floor).

`@cairn` and `@intended_targets` appear at most once per module, and a repeat is
`E_DUPLICATE_HEADER`. `@requires` is the exception: its floors compose, so repeating it adds a
constraint rather than displacing one.

A `def` or a `theme` may carry the same expression as a body line, spelled `requires` without the
`@`. It is a floor on that part rather than on the file, inherited by every build that instantiates
it. The sigil is what marks a file directive, and a part's floor is not one. See
[Versioning and Editions](/spec/versioning-editions/). Inside a body the sigil opens only an
`@edition` guard, which is not a directive either: it picks the edition whose build keeps the lines
under it ([§10.7](/spec/versioning-editions/#an-edition-guard-holds-the-escape-hatch)).

## 5.4 Selectors

Wall selectors are `front` (+z), `back`, `left`, and `right`. `offset` runs along the wall, and `y`
is measured from the floor (`y = 0`). Inside faces are prefixed: `inside.front`. Blocks, block
entities, and entities all use one selector grammar.

**`offset` origin.** `offset=0` sits at the wall's left end viewed from outside that wall. The
`front` and `back` walls anchor at low `x`, `front` from the +z viewpoint and `back` mirrored along
x so a `sym=true` opening looks symmetric from either side. The `left` and `right` walls anchor at
low `z` and mirror the same way.

`sym=true` mirrors the opening across the wall's midpoint
(`mirror_offset = wall_length - offset - size_w`). A mirror overlapping the primary rectangle
is rejected with `W_DEFERRED_MEMBER`, and only the primary is painted. That includes the
mirror of a window centred on the wall's midpoint (`2 * offset + size_w = wall_length`), which
coincides with the primary. A mirror that only touches the primary, beginning in the column after
the primary's last (`2 * (offset + size_w) = wall_length`) or ending in the column before its first
(`2 * offset = wall_length`), shares no cell with it, so it is neither rejected nor reported: both
are painted, and read as one opening twice the window's width. `sym=` takes a bare `true` or
`false`, and a window without it is not mirrored. Any other value (`sym=yes`, `sym="true"`, `sym=1`)
is an unreadable value: the window is drawn without its mirror, and the value is reported as
`W_IGNORED_ARGUMENT` ([Lint §11.3](/spec/lint/#113-error-vs-warning)). A window whose `repeat=` is
greater than 1 is the exception. Mirroring a series is reserved for a later extension, so a
`sym=true` series is not cut yet and is reported as `W_DEFERRED_MEMBER`, and an unreadable `sym=` is
not read as `false` there: that window is not cut either, and is reported as `W_DEFERRED_MEMBER`
beside the `W_IGNORED_ARGUMENT`.

**`at=` door anchors.** A door's wall-local column comes from one of three named anchors:

| Anchor | Column |
|---|---|
| `at=center` | `u = floor(wall_length / 2)`. Odd walls have a unique centre; even walls pick the column right of the midpoint. |
| `at=left` | The wall-local axis origin, `u = 0`. |
| `at=right` | The far corner, `u = wall_length - 1`. |

The same column resolves both the door and any `connect` walkway anchored to it
([§9.3.5](/spec/components-editing-sites/#935-ports-and-connect)). Numeric offsets (`at=N`) are
reserved for a future extension. A door with no `at=`, or one that names no anchor, is
`E_INVALID_MEMBER` and cuts nothing.

**Which rows a door opens.** A door under `level y=N` opens at row `N + 1`, the row above that
level's base plane, and stands two rows tall: its door block takes that row and the one above it
([Blockstate Model §6.1](/spec/blockstate/#61-derive-by-default-promote-on-override)). Both rows
MUST be inside one course of the masonry. A door whose two rows the walls as written hold in no one
course is `E_INVALID_MEMBER` and cuts nothing, the same finding the `window` on that body earns. A
door under a one-row course is refused the same way, since the row above it belongs to the roof or
to nothing. A door in a course whose `walls` paint nothing, or beside `walls` under a `level` that
is not placed, is `W_DEFERRED_MEMBER` instead, since the repair is on those walls or that level
([§9.3.5](/spec/components-editing-sites/#935-ports-and-connect) states the courses a `walls`
paints).

**What a door is built from.** A door's `mat_slot=` names its door block, and a door without one is
built from the registry pack's `@door.default`
([Materials and Themes §7.2](/spec/materials-themes/#tier-1-tokens)). The block MUST be a door, one
whose id ends in `_door`. Any other is `E_INCOMPATIBLE_MATERIAL` ([Lint](/spec/lint/)), as a sloped
roof bound outside the stair family is, and the door is built from `@door.default` beside the
error.

## 5.5 IDs, classes, addresses

**Subject to change.** The address the compiler gives a member without `id=`, and the `edit` patch
DSL that would use it, are not built yet
([Compatibility Tiers](/spec/compatibility/#subject-to-change)). Today such a member has no address,
and a top-level `edit` is `E_PARSE`.

Important members MAY declare `id=`, and `class=` groups members. Members without an `id=` get a
stable, meaning-based address assigned by the compiler, derived from parent / role / side / level /
offset. See
[Components, Editing, and Multi-building §9.2](/spec/components-editing-sites/#92-editing-model).

A `place` row is the exception: its `id=` is required, and omitting it is `E_INCOMPLETE_PLACE`. An
auto-address names nothing outside the body it sits in. A `place`'s `id=` is what `east_of=` and
`connect` refer to, and what its `.nbt` is written under, so an invented one would be a name the
author never wrote and cannot point at.
