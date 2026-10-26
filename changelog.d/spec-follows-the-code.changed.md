- *(spec)* Where the specification and the compiler disagreed and the compiler was right, the
  specification now says what the compiler does, and each such change gives its reason:
  - `@requires` needs whitespace after an edition, so `javaversion>=1.21` is `E_INVALID_REQUIRES`.
  - The lockfile is `<source>.lock` beside its source (`village.crn.lock`) unless `--lock` names
    another path, and lock schema version 1 includes the `placements` and `walkways` keys.
  - A rectangular hip roof's apex fills the top layer's inset rectangle, two rows deep on an even
    short span, where one row would leave a hole along the ridge.
  - An actuator key on a member that does not read it, misspelt or not, is `E_UNKNOWN_ARGUMENT`,
    which `cairn check` reports, rather than a finding only `cairn synth` reaches.
  - `E_THEME_SELECTOR_UNMATCHED` weighs a selector row against the scopes its theme is bound to,
    and a theme no scope binds is not checked.
  - The redstone layers are four: the Edition Netlist IR sits between the Netlist IR and the
    Placement IR.
  - `E_INVALID_PLACE_ORIGIN` covers a `place` with no single label origin; `E_MISSING_PATH_MATERIAL`
    covers `path=gravel`; `W_DUPLICATE_WALKWAY` holds in either order; `W_WALKWAY_BLOCKED` covers a
    walkway past the router's search area; and the did-you-mean note covers every closed
    vocabulary.
  - Cairn versions are `YYYY.M.PATCH` and compare as numbers; `at=center` on an even wall is
    `floor(len / 2)`; a `level`'s offset counts toward `Dims.y`; `cairn info`'s `buildable targets`
    row pins each version and can raise `E_UNKNOWN_ID`; each edition's version table holds its
    release lines; and a state the translator cannot read may come from a state literal.
  - `cairn synth`, its JSON and every code of the redstone pipeline are Internal tier.
  - A size literal's two extents are non-zero.
  - An empty `buildable` has a fifth cause, `states_unmapped`, a palette entry the edition's writer
    cannot carry.
- *(spec)* Where the compiler does not carry out a rule yet, the rule stays and a note says what
  happens today, such as `E_PARSE` for an `edit` statement or `E_UNKNOWN_KEYWORD` for an entity.
