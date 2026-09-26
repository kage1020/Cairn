---
title: "11. Lint and Constraint Validation"
---

The compiler reports warnings and errors with line numbers. Every message MUST carry the
self-correction triple: **what is wrong / valid candidates in the target / a suggested fix**. That
shape is what feeds the loop in [Evaluation Framework](/spec/evaluation/).

## 11.1 Diagnostic codes

This section is the catalog: every code a stable command can raise has a row here, and a code with
no row is a bug in this section rather than a code outside it. Where a code's rule belongs to
another chapter, the row states what the code means and links there for the rule — the row is what a
reader with a code in hand needs to find, and the chapter is where the behaviour is defined.

The redstone pipeline's `E_LOGIC_*` / `W_LOGIC_*` codes are outside that promise. They are reachable
only through `cairn synth --experimental-logic-synth`, which is Internal tier
([Compatibility](/spec/compatibility/)) — nothing about those strings is guaranteed, so a row here would
state a contract that does not exist. [Redstone and Logic](/spec/redstone/) names the ones its own rules
turn on.

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
partly overlap (see [Materials and Themes §7.1](/spec/materials-themes/#71-slots-as-dependency-injection)).

`E_DUPLICATE_ITEM` treats `theme` / `def` / `struct` / `site` as four separate namespaces, so one
name may appear once in each. For the first three the first declaration resolves and the rest bind
nothing. Two `site` blocks of one name instead merge into a shared `site::NAME::PLACE_ID`
namespace: every place with a distinct `id=` still builds, only a repeated `id=` collides, and
`east_of=` does not reach across the blocks.

`E_DUPLICATE_HEADER` covers `@cairn` and `@intended_targets`. `@requires` is excluded: its floors
compose to the strictest across every line, so a second one adds a constraint ([§5.3](/spec/syntax/#53-headers)).

### Syntax and structure

| Code | Meaning |
|---|---|
| `E_PARSE` | The source did not parse. |
| `E_UNKNOWN_KEYWORD` | The statement keyword is not in the known-keyword table. |
| `E_UNKNOWN_ARGUMENT` | A `key=` outside the vocabulary of the member's keyword, written as an argument or inside the member's own `[key=value]`. |
| `W_IGNORED_ARGUMENT` | A `key=` inside that vocabulary that no pass read on the line it was written on. |
| `E_MISPLACED_BINDING` | A `-> value` tail on a member whose keyword cannot emit a signal ([§14.2](/spec/redstone/#142-signal-binding)). |
| `E_MISPLACED_MEMBER` | The keyword is known, but the enclosing body has no reader for it. |
| `E_UNEXPECTED_POSITIONAL` | A bare value on a line that reads none ([§5.1](/spec/syntax/#51-lexical)). |
| `E_UNSUPPORTED_NESTING` | A member carries an indented body that nothing reads. |
| `E_TYPE_MISMATCH_LABEL` | A label-typed key's value is not an identifier or string. |
| `E_TYPE_MISMATCH_SIZE` | A `size=` value is not a `WxH` literal. |
| `E_CONNECT_ARITY` | A `connect` row's shape is not `FROM.PORT to TO.PORT`. |
| `E_INVALID_REQUIRES` | A `requires` expression that is not a version floor, written as the `@requires` header or as a `def` / `theme` body line ([§5.3](/spec/syntax/#53-headers)). |
| `W_INVALID_CAIRN_VERSION` | An `@cairn` value that is not a `YYYY.M[.PATCH]` language version ([§5.3](/spec/syntax/#53-headers)). |
| `W_FUTURE_CAIRN_VERSION` | An `@cairn` value naming a language version later than the compiler reading it. |

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

`E_INVALID_REQUIRES`: the accepted shape is an optional edition, `version`, `>=`, and a version
label, with whitespace optional. The code covers a word before `version` that is not an edition,
any other operator, a missing version, a component that does not begin with a digit or does not fit
in a `u32`, a `-` with no readable pre-release tag after it, and text after the version. It does
*not* cover a well-formed label the target edition has no `DataVersion` for — that is
`E_REQUIRES_UNORDERABLE`, and it is not a syntax fact. Both spellings of the floor are checked, and
by the same rule: the `@requires` header, and the member-level `requires` line a `def` or a `theme`
may carry ([§10.4](/spec/versioning-editions/#a-part-may-declare-its-own-floor)). A part no build
instantiates is checked too — the mistake is in the line, not in whether anything reads it.

`W_INVALID_CAIRN_VERSION`: the accepted shape is `YYYY.M` or `YYYY.M.PATCH` — a four-digit year, a
month `1 … 12`, and an optional patch, every component decimal digits. A leading zero on the month
is accepted and does not change it: `2026.06` and `2026.6` are one version, the first being
calver.org's `YYYY.0M` and the second the `YYYY.M` a Cargo `version` field can carry. The code
covers a value that is not two or three components, a component that is empty or not digits, a
year that is not four digits, a month outside the calendar, a patch past `u32`, and a second word
after the version — the header value is a whole line, so `@cairn 2026.6 draft` names a version and
then something else.

This is deliberately stricter than `@requires`, which reads a Minecraft label out of Mojang's
namespace and only has to order it, so a component there need only *begin* with a digit and a
pre-release tag is a label too. `@cairn` names Cairn's own version, so every component is digits,
`2026.13` is a month that does not exist and `1.2` is a semver rather than a year.

`W_FUTURE_CAIRN_VERSION` is the one thing a compiler can usefully say about a file written against
a language newer than itself: a keyword or argument added after this build is reported as
`E_UNKNOWN_KEYWORD` or `E_UNKNOWN_ARGUMENT`, and only the header knows those findings may be about
the version gap rather than about the lines they name. That reaches what a later language adds
*within* the shapes this one has. A whole new syntactic form — a directive, a top-level item — is
`E_PARSE`, and parsing precedes every check pass ([§11.3](#113-error-vs-warning)), so this finding
cannot appear in the case the version gap explains best.

There the header is read out of the text — one line, matched by a leading `@cairn` and read to end
of line — and `E_PARSE` carries a note saying the file declares a later language than the build
reading it. A note rather than a second finding, because a source that does not parse reports
`E_PARSE` alone and that stays true. Only a value that reads as a version earns it: a malformed one
is `W_INVALID_CAIRN_VERSION`'s business, and repeating that on an unrelated parse failure would be
noise.

The two codes never both fire on one directive — a value that is not read as a version has no
version to compare.

### Materials and targets

| Code | Meaning |
|---|---|
| `E_UNKNOWN_ID` | A resolved block ID the pinned target does not declare. |
| `E_VERSION_CAP` | `--target` is below an `@requires` floor the source declares ([versioning-editions §10.4](/spec/versioning-editions/)). |
| `E_REQUIRES_UNORDERABLE` | An `@requires` floor names a version the target edition's `DataVersion` table cannot place ([versioning-editions §10.4](/spec/versioning-editions/)). |
| `E_INTENDED_TARGET_CAP` | Every version `@intended_targets` names is below a floor the same file declares. |
| `W_INTENDED_TARGET_CAP` | Some, not all, of them are. |
| `W_INTENDED_TARGET_UNSUPPORTED` | `@intended_targets` names a version no `--target` of the edition can build. |
| `E_INCOMPATIBLE_MATERIAL` | A member whose geometry attaches blockstates is bound to a material that cannot carry them. |
| `E_MISSING_MATERIAL` | A member whose only route to a block is `mat_slot=` was written without one. |
| `E_UNRESOLVED_SLOT` | A member's `mat_slot=` names a slot the bound theme does not declare. |
| `E_UNKNOWN_SLOT_TARGET` | A `slot NAME -> VALUE` whose value is neither a canonical nor an abstract material token ([Materials and Themes](/spec/materials-themes/)). |
| `E_THEME_SELECTOR_UNMATCHED` | A `theme` selector row that matches no member in the file. |
| `E_THEME_VARIANT_MISSING` | The pinned edition can bind none of a theme's per-edition variants. |
| `E_INCOMPLETE_PLACE` | A `place` row omits `id=`, `use=`, or `theme=` ([§9.3](/spec/components-editing-sites/#93-multi-building-with-site)). |
| `E_UNKNOWN_ABSTRACT_TOKEN` | A `mat_slot=` resolves to an abstract material token the offered pack's catalog does not declare ([Materials and Themes](/spec/materials-themes/)). |
| `W_ABSTRACT_TOKEN_DEFERRED` | The same token with no catalog offered at all, so there is nothing to lift it against. |
| `W_NO_THEME_BOUND` | A scope has no theme bound to it, so every `mat_slot=` member in it lowers to air. |
| `W_THEME_VARIANT_REBOUND` | A `place theme=` names one edition's variant and the pinned edition bound a different one ([Versioning and Editions](/spec/versioning-editions/)). |

`E_UNKNOWN_ID` and `E_INCOMPATIBLE_MATERIAL` are raised during block-array lowering, so only the
commands that lower report them: `cairn compile`, `cairn lower`, `cairn info`, and `cairn check
--edition E --target V`. `E_UNKNOWN_ID` further needs a pinned target, so the two commands that
raise it are `cairn compile --target` and `cairn check --edition E --target V` — `info` and
`lower` lower against no version. A `cairn check` with no `--target` runs no lowering at all and
reaches neither code.

`cairn check --target` exists so a CI job can gate on the check command and still see the
lowering-stage findings a compile would refuse on: an id the target does not declare passed
`check` at exit 0 and stopped `cairn compile` at exit 1, and the information that decides it — the
one `(edition, version)` pair — was not on `check`'s command line. It requires `--edition` for the
reason [Compilation Model §4.2](/spec/compilation/#42-target-axes) refuses `--target` alone, and runs the
same lowering pass against the same table `compile` does, so a lost scope earns the same
`E_PARTIAL_BUILD`. What it does not do is anything `compile` writes: no artifact, no lockfile, and
no `@requires` floor enforcement — a lock certifies a build, and only the command that produces
one holds `--target` to the floors (`E_VERSION_CAP`). Leaving the flag off is unchanged behaviour,
so no source that passes today starts failing. See
[Versioning and Editions §10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference).

`E_INCOMPATIBLE_MATERIAL` today means a sloped roof or an eave `stair` bound outside the stair
family ([Compilation Model §4.3](/spec/compilation/#43-gable-roof-voxel-rules)).

`E_THEME_VARIANT_MISSING` fires only under `--edition`, and is reported **once per logical theme**
however many scopes read it, since they all want the same edit in the same `theme` block. Every
placement naming it is still refused. A module that declares such a theme but never reads a
`mat_slot=` from it is not reported: the build is byte-identical with or without the pin.

`E_THEME_SELECTOR_UNMATCHED` is a warning despite its prefix. A rule that matches nothing overrides
nothing, so every member keeps the material it would have had with the rule deleted; the finding is
about the author's intent rather than about what was built. The prefix is part of the code string,
which is Stable ([Compatibility Tiers](/spec/compatibility/)), so it stays as written — read severity from
the `severity` field rather than from the first letter.

`E_UNKNOWN_SLOT_TARGET` is an error on the opposite test: a slot bound to nothing lowers every
`mat_slot=` pointing at it to air, so a theme whose slots are all mistyped builds a hollow shell of
the requested extent at exit 0.

`E_MISSING_MATERIAL` and `E_UNRESOLVED_SLOT` are the two halves of one split, and no member
earns both: the key is absent for the first and present-but-unusable for the second. They have
different repairs, which is why they are different codes — the same division `E_INCOMPLETE_PLACE`
and `E_TYPE_MISMATCH_LABEL` draw on a `place` row.

`E_MISSING_MATERIAL` applies to the roles that put nothing anywhere without one. `floor` and
`walls` reach the palette through the applied theme's slot map and have no default block, so a
bare one contributes no voxel. A `window` without a `mat_slot=` is an **opening**, carved to air,
and is not reported — that is how a narrow slit is punched through a wall without choosing a
species for it; a `door` is always a carve and reads no material at all. A `roof`, `stair` or
`pressure_plate` paints a default block and is not reported either. `cairn check` raises it, before any lowering, and it needs no theme: a module
that declares none is still told which of its members name no material.

`E_UNRESOLVED_SLOT` is reported **at most once per member per bound theme**. A `def` body is
resolved once as its own scope and once again for every `place` that instantiates it, and each of
those resolutions binds a theme; a repeat against a theme already reported for that member is
dropped. Two placements naming two themes are two findings: each names the theme it is about and
each is a separate edit. Which resolution a surviving finding came from is not specified, because
two of them can bind the same theme and still judge the slot differently.

The difference is edition-variant softening. Without an `--edition` pin, a slot that any sibling
variant of the picked theme declares counts as known and is not reported: the concrete binding is
edition-specific and comes into scope only once a pin narrows the theme to one variant
([Versioning and Editions §10.7](/spec/versioning-editions/#107-java--bedrock-portability)). A
`place ... theme=NAME` is softened the same way only when `NAME` names the logical theme — naming
a variant asks about that variant's slots alone. Two placements can therefore bind one theme and
disagree about one slot.

A `def` nothing places is still resolved, against the theme the module picks, so a file of defs
and no `site` is checked as long as the module has a single logical theme to pick. Where it
declares more than one — or none at all — no theme binds to the def's own scope and its
`mat_slot=` names are not judged until a `place` chooses one.

`E_INCOMPLETE_PLACE` names every key the row is short of, and the row is dropped from the build.

`E_UNKNOWN_ABSTRACT_TOKEN` and `W_ABSTRACT_TOKEN_DEFERRED` differ on whether anything could have
answered. A pack was offered and does not declare the token, so the build stops with a suggestion
towards the closest one it does declare; no pack was offered, so nothing was asked and the cell
degrades to air with a warning. The second is the path a library caller reaches — LSP highlighting,
or a `cairn check` with no pack — and it is a warning for that reason: refusing there would refuse
every source read without a pack.

`W_NO_THEME_BOUND` is the same shape one level up: a `mat_slot=` that resolves against no theme at
all has no slot map to read, so the member contributes no voxel. A module that binds no theme and
reads no `mat_slot=` is not reported.

The three `@intended_targets` codes weigh the file's stated intent against its own floor
([versioning-editions §10.4](/spec/versioning-editions/#the-hint-is-weighed-against-the-floor)). A version
the edition cannot build is `W_INTENDED_TARGET_UNSUPPORTED` and is not also weighed against a floor:
"this target does not exist here" is what the author acts on, and a cap beside it would send them to
edit a line that is not what stops the build. The rest — the versions the edition *can* build — are
counted among themselves: *every* one of them below a floor is `E_INTENDED_TARGET_CAP`, since the
file can then be built for nothing it says it is for and the first `cairn compile --target` naming
any of them is `E_VERSION_CAP`, while *some* of them is `W_INTENDED_TARGET_CAP`, because the header
is a hint ([§5.3](/spec/syntax/#53-headers)) and the versions above the floor still build. A version that
was never buildable is not in that count either way: it answers for none of the list, and letting it
would report a file nothing can build as half a problem. One header can earn a cap code and the
unsupported code at once; the versions it names are not all wrong in the same way.

All three are per-edition answers, since a floor and a target label are ordered in one edition's
`DataVersion` table. Every command that gates on `cairn check` reports the two cap codes — `check`,
`info`, `lower`, `compile`, `synth` — each weighing the header in the tables of the editions it is
about: the one `--edition` names, the ones `cairn info --editions` lists, or both where the command
names none. A finding either edition reaches is reported, because the contradiction is between two
lines of the file however it is later built; one span carries one cap finding, and two editions
disagreeing about how far it reaches report the error. `W_INTENDED_TARGET_UNSUPPORTED` waits until
exactly one edition is in scope: a version Java cannot build is routinely the Bedrock target the
author means, so with both in scope the question has not been asked.

### Sites and placements

| Code | Meaning |
|---|---|
| `E_INVALID_PLACE_ID` | A `place id=` is empty or carries `.`, `:` or whitespace. |
| `E_DUPLICATE_PLACE_ID` | Two `place` rows in one site share an `id=`. |
| `E_INVALID_PLACE_ORIGIN` | A `place` carries an `at=` other than `origin`, or combines `at=` with `east_of=` / `north_of=` ([§9.3](/spec/components-editing-sites/#93-multi-building-with-site)). |
| `E_UNRESOLVED_PLACE_REF` | A `place use=`, an `east_of=` / `north_of=`, or a `connect` endpoint names a place or def that does not exist. |
| `E_UNRESOLVED_THEME_REF` | A `place theme=` names a theme the module does not declare. |
| `W_UNUSED_DEF` | A `def` no `place use=` references. |

`E_INVALID_PLACE_ID` is about round-tripping rather than taste. The scope key
`site::SITE::PLACE`, and every walkway key parsed back out of one, is built from those characters
as separators, so an id carrying one cannot be read back. `id=` accepts a string literal, which is
what let the value through.

`E_DUPLICATE_PLACE_ID` names both spans. The first row wins for everything that references the id
and the duplicate is dropped, so a reference resolving to "the other one" is not a second finding.

`E_UNRESOLVED_PLACE_REF` and `E_UNRESOLVED_THEME_REF` each carry a nearest-match suggestion when
one fits the spell cap ([did you mean](#did-you-mean)). Both are errors because substituting
something for the name would build a site the source did not describe.

### Connections and walkways

| Code | Meaning |
|---|---|
| `E_UNRESOLVED_PORT` | A `connect A.PORT to B.PORT` names a port the referenced def does not expose. |
| `E_AMBIGUOUS_PORT` | The port id matches more than one member of the referenced def. |
| `E_MISSING_PATH_MATERIAL` | A `connect` row carries no `path=`, so the walkway has no material to lay. |
| `W_DUPLICATE_WALKWAY` | A `connect` repeats a `(from, to)` pair an earlier row in the same site already laid. |
| `W_INVALID_WALKWAY_IDENT` | A site, place or port identifier in a `connect` contains `__`. |
| `W_DEFERRED_CONNECT` | A `connect` targets a `place` that was itself refused, so there is nothing to connect. |
| `W_WALKWAY_BLOCKED` | Cells of the fallback path overlapped an existing structure and were dropped. |

`E_UNRESOLVED_PORT` is the port half of the `place.port` shape alone; the place half is
`E_UNRESOLVED_PLACE_REF`. `E_AMBIGUOUS_PORT` takes the first match for lowering and reports the
collision, since the repair is to rename one of the colliding members rather than to pick for the
author.

`E_MISSING_PATH_MATERIAL` is an error where the other absent-material codes are warnings: a walkway
that degrades to air leaves two buildings looking connected in the source and unconnected in the
world, with nothing in the report to say so.

`W_INVALID_WALKWAY_IDENT` is the same round-trip rule as `E_INVALID_PLACE_ID` on a different
separator. `__` joins the `from` and `to` halves of a walkway's scope key, so `b__c` in one half and
`c__home2` in the other encode to one string. The row is dropped and the finding names the segment
to rename.

`W_DEFERRED_CONNECT` follows whatever refused the `place` — an incomplete row, a mistyped key, a
failed origin selector, an unresolved `use=` or `theme=`. It is a warning because the finding that
has the repair is the one on the `place`, and reporting the `connect` as a second error would send
the author to a line that is correct.

### Lowering

| Code | Meaning |
|---|---|
| `W_DEFERRED_MEMBER` | A member the block-array pass does not lower, so the scope builds without it. |
| `W_STRUCT_NO_SIZE` | A `struct` declares no `size=WxH`, so lowering can derive no extent and skips it. |
| `W_DEF_NO_SIZE` | The same on a `def`, so every `place use=` of it is skipped. |
| `W_STRUCTURE_TOO_LARGE` | A scope's derived extent exceeds the volume the block-array pass will allocate for. |
| `W_PHASE_CONFLICT` | Two members in one phase wrote one voxel to different blocks ([§4.4](/spec/compilation/)). |
| `E_PARTIAL_BUILD` | At least one requested scope did not lower, so the run produced less than was asked for. |

`W_STRUCT_NO_SIZE` and `W_DEF_NO_SIZE` are one rule split by what carries it, so a filter matching
on `code` can tell a struct that will not build from a template that will not instantiate.

`W_STRUCTURE_TOO_LARGE` fires on a *combination*: `size=`, `walls height=`, `roof overhang=` and
`level y=` are each range-checked on their own, and this is the product of them being out of reach.
A warning rather than an error, matching the two above — the scope is skipped and the rest of the
build is unaffected.

`W_DEFERRED_MEMBER` keeps a partial build inspectable rather than failing the module: the rest of
the scope lowers, and the finding names what is missing from it.

`E_PARTIAL_BUILD` is the run-level counterpart, and the one error among these: a warning above says
a scope builds without something, and this says a scope the command was asked for did not build at
all. It is reported once for the run, naming how many of the requested scopes were lost, by
`cairn compile` and by a `cairn check --edition E --target V` that runs the same lowering pass.

`W_PHASE_CONFLICT` is last-wins reported rather than refused. [Compilation Model](/spec/compilation/)
grants last-wins to local overrides within one phase, which is what an author restating a member
is; two footprints that happen to intersect is not, and the grid cannot tell the two apart. The
resolution the spec mandates still happens — the finding says which voxel it happened at.

### Truth tables

| Code | Meaning |
|---|---|
| `E_TRUTH_TABLE_EMPTY` | An `assert truth(...)` with no rows, or none whose output is `0` or `1`. |
| `E_TRUTH_TABLE_CONFLICT` | Two rows assign the same input combination different outputs. |
| `W_TRUTH_TABLE_DUPLICATE_ROW` | Two rows cover the same input combination without contradicting each other. |
| `W_TRUTH_TABLE_PARTIAL` | The rows leave input combinations unassigned. |

Both codes are reported on the later row, with a note at the first row assigning that combination.
The spec does not say which of two conflicting rows an evaluator would read, because the repair is
to decide which row is wrong.

A `-` makes the same combination reachable from rows that do not look alike, so both codes are
about the combination rather than about the pattern: `0-` and `-1` both assign `01`. The fix
differs with the shape. A row inside an earlier one — `01` under `0-` — is deleted. Two rows that
merely cross are narrowed, because deleting either would lose the combinations only it assigns.

A `-` **output** is the one shape that is neither a conflict nor a repeat: the row declines to
constrain its combinations, so there is nothing for a concrete output to contradict, and nothing
for it to agree with either. The pair is `W_TRUTH_TABLE_DUPLICATE_ROW`, and its fix names the
asymmetry — whichever of the two rows is deleted, the table reads differently afterwards. A table
with rows but no `0` or `1` output among them constrains nothing, which is why
`E_TRUTH_TABLE_EMPTY` covers it too; the sentence differs from the no-rows one, because the repair
is to change a row rather than to add one.

The two warnings are warnings because every row present is still a real constraint. One table can
earn both `W_TRUTH_TABLE_DUPLICATE_ROW` and `W_TRUTH_TABLE_PARTIAL`: a repeated row, or one inside
an earlier one, fills no combination the table did not already have. Two rows that cross are the
exception — the coverage finding is withheld there, because a count taken without the later row
would name a combination the table does assign.

### Semantic categories

Beyond the codes above, lint covers:

| Category | Checks |
|---|---|
| **Geometry** | AABB expansion: a window outside the wall, a door hanging in mid-air. |
| **attachment** | A frame, painting, sign, button, lever, or torch on a valid attachment face. |
| **entity_aabb** | Entities not clipping walls or paths, not blocking a door's swing, not cramming. |
| **support** | Hanging lanterns, torches, campfires, and gravity blocks such as gravel. |
| **fluid** | Consistency of water source, flow, and `waterlogged`. |
| **version_caps / parity** | Whether a state or entity schema is usable in the target ([Versioning and Editions](/spec/versioning-editions/)). |
| **edit_stability** | Whether an `intent_state` change ripples into an unrelated member's `resolved_state`. |
| **redstone** | Per-tick simulation against the declared truth table and temporal assertions; timing conflicts, QC dependence, routing congestion ([Redstone](/spec/redstone/)). |
| **AABB interference** | On overlap, priority-merge or reject. Boundary blockstate re-resolution is the IR layer's job. |

### "did you mean"

Three diagnostics reject an identifier against a closed vocabulary: an unknown keyword, an unknown
`mat_slot=` name, and an unknown `--target` version. Each attaches a ``did you mean `X`?`` note when
a candidate sits within a length-scaled Damerau-Levenshtein cap of ≤ 1 edit for 1–3 characters, ≤ 2
for 4–6, and ≤ 3 beyond. The closed-set listing (`expected one of: ...`) is always printed as well.

## 11.2 Machine-readable payload

Every command that takes `--format json` — `check`, `info`, `parse` and `lower` — writes exactly one
JSON document to stdout under it, for every input it is given. `check`'s document is an array of findings, so a source that does not parse is
that array with one `E_PARSE` element. `info`'s is the report; where there is no report — a parse
failure, or any error-severity finding — it writes `{"diagnostics": [ ... ]}` instead, told apart
from a report by its keys and by the exit code. Which pass raised the finding does not change that:
the strict per-edition dry-run sees what the edition-neutral gate unions away, and a refusal it
raises writes the same document, after every requested edition has been walked.

Which pass raised the finding does decide what the document carries, and this is the contract rather
than an accident. Where the edition-neutral pass refuses, its warnings are elements of the document
beside the errors. Where the per-edition pass refuses, the document carries the errors alone: the
neutral warnings have already been reported as text, and a per-edition warning belongs to a row the
run is about to discard, so it reads on stderr under the note naming its edition. Warnings on a run
that still has a report are reported as text on stderr in both formats.

A run-level refusal is not an element of `check`'s array: an unshipped `--target` and a lowering
that lost a scope (`E_PARTIAL_BUILD`) are facts about the command line and about the build rather
than findings at a span, so both are reported on stderr and by the exit code in either format —
the shape `compile` gives them. The array still carries every finding the run did reach, so it is
a report of what was checked rather than an empty document.

This is settled rather than pending: `check --format json` has no machine-readable form for a
run-level refusal, and a consumer reads the verdict from the exit code. Stderr is prose for a person
in either format. It is not part of this contract, and a consumer should treat it as unstructured
text rather than parse it. What the consumer can tell without it is *that* the run was refused at
the run level rather than by a finding: an exit of `1` over an array that holds no element with
`"severity": "error"` means exactly that, down to a `[]` over a source with nothing to report. No
other failure reads that way — a source that does not parse is an array carrying `E_PARSE`, and a
file that cannot be read writes no document at all. Which of the two refusals it was, and what the
build lost, is said only on stderr.

One `info` refusal is a run-level refusal of the same kind: a registry pack whose palette carries a
blockstate the pack was expected to refuse costs that edition its portability row, and names no span
in the source and no repair its author could make. It reads as prose on stderr in both formats.
The document is still written — the promise is one document per input, not one element per
refusal — so a run refused by nothing else writes `{"diagnostics": []}` and says the rest with its
exit code.

`parse`'s product is the AST and `lower`'s is the block-array IR. A dump is not a report, so a
failure is not the dump with a hole in it: each writes the same `{"diagnostics": [ ... ]}` document
`info` does, told apart from the dump by its keys and by the exit code. `lower` writes it for a
source that does not parse and for one that fails a later pass, since neither leaves an IR worth
dumping. Under every other `--format` the findings read as prose on stderr.

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
| `E_UNKNOWN_ID` | `{ "kind": "unknown_id", "id", "registry", "origin", "token"?, "suggestion"?, "aliases"? }`. See below. |
| `E_INCOMPATIBLE_MATERIAL` | `{ "kind": "incompatible_material", "id", "required", "slot"?, "token"? }`. The bound material, the family the geometry needs, and where the binding came from. |
| `E_INCOMPLETE_PLACE` | `{ "kind": "incomplete_place", "missing": ["id", "use", "theme"] }`. The keys the row does not declare. Never empty. |
| `E_INVALID_REQUIRES` | `{ "kind": "invalid_requires", "reason", "found" }`. `reason` is one of `not_a_version_requirement`, `unknown_edition_scope`, `unsupported_operator`, `empty_version`, `component_not_a_number`, `component_too_large`, `prerelease_not_a_tag`, `trailing_tokens`. `found` is empty when the failure names no fragment. |
| `W_INVALID_CAIRN_VERSION` | `{ "kind": "invalid_cairn_version", "reason", "found" }`. `reason` is one of `component_count`, `component_not_a_number`, `year_not_four_digits`, `month_out_of_range`, `patch_too_large`, `trailing_tokens`. `found` is the component the reason is about, and is empty when the failure names no fragment — `component_count`, and `component_not_a_number` on a value whose component is itself empty (`2026.`). |
| `W_FUTURE_CAIRN_VERSION` | `{ "kind": "future_cairn_version", "declared", "compiler" }`. Both verbatim: `declared` is the string in the file, `compiler` is the build that reported it. |
| `W_TRUTH_TABLE_PARTIAL` | `{ "kind": "truth_table_partial", "inputs": 2, "covered": 1, "missing": ["01","10","11"] }`. See below. |
| The three `@intended_targets` codes | `{ "kind": "intended_targets", "edition", "targets": ["1.20.4"], "floor"? }`. The versions this finding is about, in source order, and the edition that weighed them. `floor` is the first one that refuses them, as written; absent for `W_INTENDED_TARGET_UNSUPPORTED`, which is not about a floor. |

**`E_UNKNOWN_ID.origin`** says who chose the ID, because the repair differs:

| `origin` | Meaning | Where the fix goes |
|---|---|---|
| `authored` | The source names the ID. | The author's line. |
| `catalog` | The registry pack maps a token onto it. | The pack's mapping. |
| `builtin` | The pack declares no row for a member default, so the compiler's own ID was used. | The pack, which has to grow the row. |

`token` accompanies `catalog` and `builtin` and is absent for `authored`. `suggestion` is absent
when no declared ID is within the typo threshold. A rename is normally past it, but the two fields
are filled by independent rules and a renamed ID near its replacement carries both.

`aliases` is the other half, and the two are different claims about the same ID. A suggestion is a
guess from a string distance; an alias is the registry pack's `aliases` component stating that two
names are one block, so a quick-fix may apply an alias unasked and should not apply a suggestion.
It holds every ID the target declares for that block — the closed set, never a pick from it — in
the pack's own order, and is absent when the pack names none. The rendered note prints the first
few and counts the rest; the payload is where the whole set lives. See
[Versioning and Editions §10.4](/spec/versioning-editions/#104-fail-loud-and-minimum-version-inference).

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

The `E_` / `W_` prefix is not the severity, in either direction. Most `W_` codes mark a
partial-build degradation, but `W_UNUSED_DEF`, `W_TRUTH_TABLE_PARTIAL` and the two `@cairn` codes
are warnings without being one; and two `E_`-prefixed codes are decided by the rule above rather
than by their name:

- `E_UNKNOWN_SLOT_TARGET` is an **error**, because a slot bound to a non-material value lowers
  every member referencing it to air.
- `E_THEME_SELECTOR_UNMATCHED` is a **warning**, because a rule that matches nothing overrides
  nothing.

`E_PARSE` is one code for every shape a parse fails in — a stray character, an odd indent, an
integer past `i64`, a keyword where an item was expected — with the message saying which. A code per
shape would make every future one a line of this table, for a distinction nothing branches on; what
that costs is that a consumer reads the message to tell them apart. It is also the only code no
check pass raises: parsing precedes them all, and a source that does not parse reaches none of
them, so it is the one finding a build reports alone.

`E_MISSING_MATERIAL` is an **error** by the first rule above rather than the second: the member is
dropped from the build and nothing is put in its place, which is implicit dropping and not a
partial-build degradation. The incomplete side is the source, which named no material, and not
the compiler. The drop does not stop at the member either — a dropped `walls` lowers the wall
height the volume is derived from, so the structure shrinks and every `door` and `window` that
was to be cut into it is deferred as well.

The two `@cairn` codes are **warnings** by the same rule read the other way. The header is
provenance: no pass branches on it, no palette entry comes from it, and the lockfile's
`cairn_version` records the compiler rather than the file. `@cairn banana` builds byte-for-byte
what `@cairn 2026.06` builds, so nothing about the result is unintended. What is lost is the
header's own job — being readable by a later compiler — which is worth a word and not worth
refusing the file over. `@requires` is an error on the same test because its floor *is* an input:
it sets `cairn info`'s compatible range and the bound `cairn compile --target` is held to, so a
floor that evaporates accepts a target it should not.

`E_UNKNOWN_ARGUMENT` is an **error** for the same reason `E_UNKNOWN_KEYWORD` is, one level down.
A key outside the keyword's vocabulary names nothing, so no pass will read the value however the
compiler grows, and the member is built without whatever was being asked for. A misspelled argument
that has a default is the worst of them: the build succeeds, at the default, and says nothing.

Each keyword's vocabulary is closed, and a `theme` selector widens the one it names — writing
`window[tags=...]` in a theme makes `tags=` a key something reads on a window, and on nothing else.
The reverse direction is `E_THEME_SELECTOR_UNMATCHED`. A selector coins words; one edit away from a
word the keyword already has is a typo written twice rather than a coinage, and is refused with the
suggestion.

A member's own `[key=value]` answers to that same vocabulary. `window[clas=outer]` is the same
defect as `window clas=outer` — a word the author expects something to read that nothing does, with
the `class` lost either way — so it earns the same code and the same suggestion. What the selector
*means* is a separate question and is not settled here: a member's selector is carried through
verbatim and later passes decide whether one binds a fresh id or references an existing member.
This check does not need that answer, because the word is one something reads or it is not,
whichever the answer turns out to be. Note that a key in a member's own bracket does not make the
member *carry* the attribute, so a `theme` row selecting on it matches nothing.

`W_IGNORED_ARGUMENT` is a **warning**, and covers three things. An **unreadable value**: a `key=`
in the vocabulary whose value the pass cannot read is dropped and a default put in its place. An
**unreached key**: a `key=` this specification defines that no pass reads yet — `window shape=` /
`anchor=` and `roof footprint=` / `bounds=` are those keys today — is carried into the IR and never
consulted. And a key **routed past**: one the keyword reads only under some ways of writing a
sibling argument, on a member that writes it another way. The boundary is the keyword: a
spec-defined key on a keyword the compiler knows is reported this way, while a spec-defined
*keyword* it does not know is `E_UNKNOWN_KEYWORD` and its arguments are not judged at all. All three
make the build differ from the source. The rule forbids *silent* substitution, and all three are
announced. For the unreached key the gap is the compiler's rather than the source's, which is why
it is not a refusal. Whether autofix is offered is up to the implementation.

The routed-past shape is the vocabulary's second axis: closed per keyword *and* per the way the
argument that selects the lowering rule is written. `roof slope_to=` is read by the `kind=shed`
rule and by none of the others, so

```
roof kind=gable slope_to=front
```

builds a roof that ignores the direction it was pointed in — the same silent drop a key outside the
vocabulary makes, one argument down. It stays a warning rather than becoming the refusal
`E_UNKNOWN_ARGUMENT` is, because the repair is not decided: the key names something real on a
keyword that reads it, and either the `kind=` is the argument that was meant or the `slope_to=` is
left over. The message names both sites and picks neither.

An arm of that axis can also be the selector's *absence*, where that is itself a rule rather than a
mistake. `place gap=` is the case: a row with no `at=` is placed relative to another and reads the
distance, and `at=origin` is anchored absolutely and does not, so `at=origin gap=5` is the same
silent drop ([§9.3.2](/spec/components-editing-sites/#932-origin-selectors)).

Where the **selector** names no rule at all — a value the dispatch does not know, or, on an axis
with no absent arm, nothing written — no finding is raised here: that member lowers to nothing and
`W_DEFERRED_MEMBER` is the whole repair, so a second finding about the argument that rule would not
have read bills one repair twice. That deferral is raised during block-array lowering, so a `cairn
check` with no `--edition` / `--target` reports neither ([§11.1](#111-diagnostic-codes)); the case
that is always reported is the one where the member builds. A key another key makes inert *without*
selecting a rule is a different shape again and is not reported today: `window step=` at `repeat=1`
is a condition on a count rather than on a rule.

`E_MISPLACED_BINDING` is the fourth field of a member line asked the same question the other three
are: is this word read by anything. A `-> value` tail is read by exactly one thing — the sensor set
of [§14.2](/spec/redstone/#142-signal-binding) — so a tail on a member that is not a sensor is carried into
the IR and dropped, and the signal it names is emitted by nothing. It is an **error** on the same
test `E_UNKNOWN_ARGUMENT` meets: the build differs from the source, and no edit to the value
repairs it.

Only the host is asked here, because only the host can be asked without the Logic IR. Whether the
tail's value names a signal, and whether that signal is driven twice or by nobody, are questions the
redstone pipeline answers ([§14.2](/spec/redstone/#142-signal-binding)) — and the host is asked first, so a
tail on a member that cannot emit earns this code and not also the value's. A member whose keyword
is not in the known-keyword table is left to `E_UNKNOWN_KEYWORD`, which is why `lever -> sig.a` —
a sensor this specification lists and the surface has not reached — is not told its host is wrong.

## 11.4 Constraint catalog

In-game constraints are cataloged and managed per version: gravity blocks, attachment conditions,
fluid flow, and disallowed attachment combinations ([Versioning and Editions](/spec/versioning-editions/)).
"A frame cannot hang on glass" lives there.
